/**
 * Worked example: the mobile-connectivity control-plane contract
 * (`RemoteConnectPanel`/managed-VM setup flow, mobile PRD Phase 2-3)
 * against a real local Supabase CLI stack, exercising the exact
 * request/response shapes the Tauri app's `ensure_mobile_device_key`
 * command and the managed-connect flow send and expect against the
 * `remote-instance` / `remote-ssh-trust` edge functions.
 *
 * Requires `TREQ_REMOTE_SPRITES_STUB=1` (set by `scripts/service-qa/up.sh`) so
 * `ensure`/`wake`/`status` resolve without a real Fly.io account.
 */
import { it, expect, afterEach } from "vitest";
import { getAnonClient, getAnonKey, getFunctionsBaseUrl, getServiceClient } from "../clients";
import { FEATURES } from "../features";
import { createTestUser, deleteTestUser, type TestUser } from "../seed";
import { clearBilling, grantPro } from "../billing";
import { recordOutcome } from "../record";

const usersToDelete: string[] = [];

afterEach(async () => {
  const admin = getServiceClient();
  while (usersToDelete.length > 0) {
    const id = usersToDelete.pop()!;
    await clearBilling(admin, [id]);
    try {
      await deleteTestUser(admin, id);
    } catch {
      // already deleted
    }
  }
});

async function signedInUser(): Promise<{
  testUser: TestUser;
  client: ReturnType<typeof getAnonClient>;
  accessToken: string;
}> {
  expect(FEATURES.emailSignup).toBe(true);
  const testUser = await createTestUser();
  usersToDelete.push(testUser.user.id);
  // ensure/wake/reprovision require Pro (027_billing_enforcement.sql).
  await grantPro(getServiceClient(), testUser.user.id);
  const client = getAnonClient();
  const { data, error } = await client.auth.signInWithPassword({
    email: testUser.email,
    password: testUser.password,
  });
  if (error || !data.session) {
    throw new Error(`sign-in failed: ${error?.message ?? "no session"}`);
  }
  return { testUser, client, accessToken: data.session.access_token };
}

/** Mirrors the mobile-connectivity flow's `invokeRemoteInstance`/
 * `invokeRemoteTrust` calls - a bare fetch against `functions/v1/<name>`
 * with `{ action, ...body }`, same as `supabase.functions.invoke` sends
 * over the wire. */
async function invoke(
  fnName: "remote-instance" | "remote-ssh-trust",
  accessToken: string,
  action: string,
  body: Record<string, unknown> = {},
): Promise<{ status: number; json: Record<string, unknown> }> {
  const response = await fetch(`${getFunctionsBaseUrl()}/${fnName}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
      apikey: getAnonKey(),
    },
    body: JSON.stringify({ action, ...body }),
  });
  const json = (await response.json().catch(() => ({}))) as Record<string, unknown>;
  return { status: response.status, json };
}

/** Minimal OpenSSH `ssh-ed25519 <base64> <comment>` line encoder, same
 * approach `supabase/functions/tests/remote_e2e.test.ts` uses, so this spec
 * doesn't need an SSH-key-format library either. */
async function generateOpenSshEd25519PublicKey(comment: string): Promise<string> {
  const keyPair = (await crypto.subtle.generateKey({ name: "Ed25519" }, true, [
    "sign",
    "verify",
  ])) as CryptoKeyPair;
  const rawPublic = new Uint8Array(await crypto.subtle.exportKey("raw", keyPair.publicKey));
  const typeBytes = new TextEncoder().encode("ssh-ed25519");
  const encodeUint32 = (value: number) => {
    const buf = new Uint8Array(4);
    new DataView(buf.buffer).setUint32(0, value, false);
    return buf;
  };
  const parts = [encodeUint32(typeBytes.length), typeBytes, encodeUint32(rawPublic.length), rawPublic];
  const total = parts.reduce((n, p) => n + p.length, 0);
  const blob = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    blob.set(part, offset);
    offset += part.length;
  }
  let binary = "";
  for (const byte of blob) binary += String.fromCharCode(byte);
  return `ssh-ed25519 ${btoa(binary)} ${comment}`;
}

it("mobile listRegions/listSizePresets return the catalog the connect flow expects", async () => {
  const { accessToken } = await signedInUser();

  const regions = await invoke("remote-instance", accessToken, "list_regions");
  expect(regions.status).toBe(200);
  expect(Array.isArray(regions.json.regions)).toBe(true);
  expect((regions.json.regions as unknown[]).length).toBeGreaterThan(0);

  const sizes = await invoke("remote-instance", accessToken, "list_sizes");
  expect(sizes.status).toBe(200);
  expect(Array.isArray(sizes.json.presets)).toBe(true);
  expect((sizes.json.presets as unknown[]).length).toBeGreaterThan(0);

  await recordOutcome("mobile-control-plane-01-catalog", {
    expectations: [
      "list_regions returns HTTP 200 with a non-empty regions array.",
      "list_sizes returns HTTP 200 with a non-empty presets array.",
    ],
    details: { regions: regions.json.regions, presets: sizes.json.presets },
  });
}, 60_000);

it(
  "mobile's ensureInstance -> registerClientKey -> issueCertificate round trip succeeds against the real edge functions",
  async () => {
    const { testUser, accessToken } = await signedInUser();

    // Mirrors ManagedConnectScreen.handleConnect: getInstanceStatus first
    // (no instance yet), then ensureInstance with a fresh idempotency key.
    const initialStatus = await invoke("remote-instance", accessToken, "status");
    expect(initialStatus.status).toBe(200);

    const ensure = await invoke("remote-instance", accessToken, "ensure", {
      idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
      region: "us_east",
      size_preset: "small",
    });
    expect(ensure.status).toBe(200);
    const ensureInstance = ensure.json.instance as { instance_id?: string } | null;
    const instanceId = String(ensureInstance?.instance_id ?? "");
    expect(instanceId).not.toBe("");

    // Poll status until the stub adapter settles on a terminal state - the
    // stub resolves fast, but this mirrors ManagedConnectScreen's own
    // schedulePollIfTransient loop rather than assuming a single poll.
    const transientStates = [
      "provisioning",
      "bootstrapping",
      "installing_access",
      "verifying",
      "waking",
      "reprovisioning",
    ];
    const deadline = Date.now() + 30_000;
    let lastStatus = "";
    let statusJson: Record<string, unknown> = {};
    while (Date.now() < deadline) {
      const poll = await invoke("remote-instance", accessToken, "status");
      expect(poll.status).toBe(200);
      statusJson = poll.json;
      const instance = poll.json.instance as { status?: string } | null;
      lastStatus = String(instance?.status ?? "");
      if (!transientStates.includes(lastStatus)) break;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    expect(lastStatus).toBe("ready");

    // registerClientKey: the app's own `ensure_mobile_device_key` Tauri
    // command produces a real ed25519 keypair via platform keystore; this
    // spec's stand-in is the same OpenSSH-line encoding
    // remote_e2e.test.ts uses.
    const publicKey = await generateOpenSshEd25519PublicKey(`${testUser.email}@mobile-qa`);
    const register = await invoke("remote-ssh-trust", accessToken, "register_client_key", {
      idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
      public_key: publicKey,
      comment: "Tauri mobile device",
    });
    expect(register.status).toBe(200);
    const registeredKey = (register.json.key ?? (register.json.keys as unknown[] | undefined)?.[0]) as
      | { id?: string }
      | undefined;
    expect(registeredKey?.id).toBeTruthy();
    const keyId = registeredKey!.id!;

    // issueCertificate: the exact request shape the managed-connect
    // flow's issueCertificate/handleConnect send.
    const issue = await invoke("remote-ssh-trust", accessToken, "issue_certificate", {
      instance_id: instanceId,
      key_id: keyId,
    });
    expect(issue.status).toBe(200);
    expect(typeof issue.json.certificate).toBe("string");
    expect(issue.json.certificate).not.toBe("");
    const endpoint = issue.json.endpoint as { host_keys?: Array<{ fingerprint_sha256?: string }> } | undefined;
    expect(endpoint?.host_keys?.[0]?.fingerprint_sha256).toBeTruthy();

    // Silent renewal: the certificate renewal manager calls
    // issue_certificate again with `renewal: true` ahead of expiry.
    const renewed = await invoke("remote-ssh-trust", accessToken, "issue_certificate", {
      instance_id: instanceId,
      key_id: keyId,
      renewal: true,
    });
    expect(renewed.status).toBe(200);
    expect(renewed.json.serial).not.toBe(issue.json.serial);

    // wakeInstance: the connect flow's handleWake path against an
    // already-ready (non-suspended) instance, same as the desktop e2e
    // suite's "ordinary wake" case.
    const wake = await invoke("remote-instance", accessToken, "wake", {
      instance_id: instanceId,
      idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
    });
    expect(wake.status).toBe(200);

    await invoke("remote-instance", accessToken, "delete", {
      idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
    });

    await recordOutcome("mobile-control-plane-02-full-round-trip", {
      expectations: [
        "ensureInstance provisions and the stub adapter reaches status=ready.",
        "registerClientKey + issueCertificate return a key id, a certificate, and a trusted host key, and a renewal call returns a distinct serial.",
        "wakeInstance against a ready instance returns HTTP 200.",
      ],
      details: {
        instanceId,
        keyId,
        initialStatus: initialStatus.json,
        readyStatus: statusJson,
        firstSerial: issue.json.serial,
        renewedSerial: renewed.json.serial,
      },
    });
  },
  60_000,
);

it("issueCertificate rejects an unregistered key id, matching the connect flow's error-throwing contract", async () => {
  const { accessToken } = await signedInUser();

  const ensure = await invoke("remote-instance", accessToken, "ensure", {
    idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
    region: "us_east",
    size_preset: "small",
  });
  expect(ensure.status).toBe(200);
  const ensureInstance = ensure.json.instance as { instance_id?: string } | null;
  const instanceId = String(ensureInstance?.instance_id ?? "");

  const issue = await invoke("remote-ssh-trust", accessToken, "issue_certificate", {
    instance_id: instanceId,
    key_id: "00000000-0000-0000-0000-000000000000",
  });
  expect(issue.status).toBeGreaterThanOrEqual(400);

  await invoke("remote-instance", accessToken, "delete", {
    idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
  });

  await recordOutcome("mobile-control-plane-03-unregistered-key-rejected", {
    expectations: ["issue_certificate for an unregistered key id returns an HTTP error status."],
    details: { httpStatus: issue.status, body: issue.json },
  });
}, 60_000);

it("a deleted managed instance can be recreated with a fresh identity", async () => {
  const { accessToken } = await signedInUser();
  const ensure = () =>
    invoke("remote-instance", accessToken, "ensure", {
      idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
    });
  const instanceOf = (res: { json: Record<string, unknown> }) =>
    res.json.instance as { instance_id: string; endpoint_id: string; status: string } | null;

  const first = instanceOf(await ensure());
  const deleteKey = `mobile-qa-${crypto.randomUUID()}`;
  const deleted = await invoke("remote-instance", accessToken, "delete", { idempotency_key: deleteKey });
  expect(deleted.status).toBe(200);
  const replay = await invoke("remote-instance", accessToken, "delete", { idempotency_key: deleteKey });
  expect(replay.status).toBe(200);
  expect(replay.json.operation_id).toBe(deleted.json.operation_id);
  const afterDelete = await invoke("remote-instance", accessToken, "status");
  expect(afterDelete.json.instance).toBeNull();

  const recreated = await ensure();
  expect(recreated.status).toBe(200);
  const second = instanceOf(recreated);
  expect(second?.status).toBe("ready");
  expect(second?.instance_id).not.toBe(first?.instance_id);
  expect(second?.endpoint_id).not.toBe(first?.endpoint_id);
  const again = instanceOf(await ensure());
  expect(again?.instance_id).toBe(second?.instance_id);

  await invoke("remote-instance", accessToken, "delete", {
    idempotency_key: `mobile-qa-${crypto.randomUUID()}`,
  });

  await recordOutcome("mobile-control-plane-04-recreate-after-delete", {
    expectations: [
      "After delete, status reports no instance and a replayed delete returns the stored operation.",
      "ensure after delete creates a ready instance with a new instance id and endpoint id.",
      "A further ensure reuses the recreated instance instead of creating a second one.",
    ],
    details: { first, second, again },
  });
}, 60_000);

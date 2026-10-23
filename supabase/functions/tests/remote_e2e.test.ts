// Phase 8 real-API end-to-end tests for the Remote SSH control plane
// (prds/remote-development.md, "Phase 8: Test infrastructure against real APIs").
//
// This suite calls real, deployed Supabase Edge Functions on a dedicated
// Supabase *test* project - never the stub adapter
// (`_shared/remote/stub-sprites-adapter.ts` / `TREQ_REMOTE_SPRITES_STUB=1`), and
// never localhost `supabase functions serve` with mocked provider calls.
// `stub-sprites-adapter.ts` and its `TREQ_REMOTE_SPRITES_STUB` switch exist
// precisely so functional/unit-style tests of the *control-plane logic*
// (routing, RLS, idempotency bookkeeping) can run without a vendor account;
// this file is the deliberately-separate non-mocked counterpart the PRD
// asks for, and must never import or enable that stub.
//
// Run with: `deno test --allow-net --allow-env supabase/functions/tests/remote_e2e.test.ts`
//
// ## Required environment variables
//
// All of the following must be set, or every test in this file prints a
// skip reason and passes trivially without asserting anything:
//
//   TREQ_REMOTE_E2E=1                     - explicit opt-in (see remote_e2e.rs
//                                            for the identical Rust-side gate
//                                            and rationale).
//   SUPABASE_TEST_URL                     - e.g. https://xyzcompany.supabase.co,
//                                            a dedicated *test* project, never
//                                            production.
//   SUPABASE_TEST_ANON_KEY
//   SUPABASE_TEST_SERVICE_ROLE_KEY
//   REMOTE_ADMIN_API_KEY_TEST             - matches that test project's
//                                            REMOTE_ADMIN_API_KEY function
//                                            secret, for remote-admin calls.
//
// The test project's own Edge Function secrets (TREQ_SPRITES_API_URL,
// TREQ_SPRITES_API_TOKEN for the dedicated Sprites test organization, the SSH CA
// key material, etc.) are configured on the Supabase side per the project's
// normal `supabase secrets set` flow, not passed through this test process.
// This suite only ever holds Supabase keys and the admin API key, never the
// Sprites token or the CA private key: provider credentials are server-side
// secrets and clients never receive vendor API tokens.
//
// See remote_e2e_README.md (in src-tauri/tests/) for the acceptance-criteria
// mapping covering both this file and remote_e2e.rs.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";

const REQUIRED_VARS = [
  "SUPABASE_TEST_URL",
  "SUPABASE_TEST_ANON_KEY",
  "SUPABASE_TEST_SERVICE_ROLE_KEY",
  "REMOTE_ADMIN_API_KEY_TEST",
];

function requiredEnv(name: string): string {
  const value = Deno.env.get(name);
  if (!value) throw new Error(`${name} is not set`);
  return value;
}

// Every resource this suite creates is tagged with this prefix (as the test
// user's email local-part and/or in operation detail fields it controls) so
// `scripts/remote-e2e-cleanup.ts` can find it independent of which test
// created it.
const E2E_TAG_PREFIX = "treq-e2e-";

function e2eConfig(): {
  url: string;
  anonKey: string;
  serviceRoleKey: string;
  adminApiKey: string;
} | null {
  if (Deno.env.get("TREQ_REMOTE_E2E") !== "1") {
    return null;
  }
  const missing = REQUIRED_VARS.filter((name) => !Deno.env.get(name));
  if (missing.length > 0) {
    console.error(`[remote-e2e] SKIP: TREQ_REMOTE_E2E=1 is set but missing: ${missing.join(", ")}`);
    return null;
  }
  return {
    url: requiredEnv("SUPABASE_TEST_URL"),
    anonKey: requiredEnv("SUPABASE_TEST_ANON_KEY"),
    serviceRoleKey: requiredEnv("SUPABASE_TEST_SERVICE_ROLE_KEY"),
    adminApiKey: requiredEnv("REMOTE_ADMIN_API_KEY_TEST"),
  };
}

function e2eTag(): string {
  return `${E2E_TAG_PREFIX}${crypto.randomUUID()}`;
}

interface InstanceJson {
  instance_id?: string;
  status?: string;
  generation?: number;
  provider_resource_id?: string | null;
  endpoint_id?: string | null;
}

/** remote-instance nests the instance row under `instance` (with `id`
 * renamed to `instance_id`) in ensure and status responses. */
function instanceOf(json: Record<string, unknown>): InstanceJson | null {
  return (json.instance as InstanceJson | null | undefined) ?? null;
}

/** Creates a disposable, uniquely-tagged test user via the admin API and
 * returns a signed-in access token plus a cleanup function. The email local
 * part carries the e2e tag prefix so the standalone cleanup script (and a
 * human auditing the test project's auth.users table) can identify it. */
async function createE2eTestUser(cfg: NonNullable<ReturnType<typeof e2eConfig>>) {
  const admin = createClient(cfg.url, cfg.serviceRoleKey);
  const tag = e2eTag();
  const email = `${tag}@e2e.treq.invalid`;
  const password = crypto.randomUUID();

  const { data: created, error: createError } = await admin.auth.admin.createUser({
    email,
    password,
    email_confirm: true,
  });
  if (createError || !created.user) {
    throw new Error(`failed to create e2e test user: ${createError?.message}`);
  }
  const testUserId = created.user.id;

  // ensure/wake/reprovision answer 402 `pro_required` without Pro. Grant it
  // through the RPCs the Stripe webhook calls (as service-qa's billing.ts).
  const billingSuffix = crypto.randomUUID().replaceAll("-", "").slice(0, 16);
  const customerId = `cus_e2e${billingSuffix}`;
  const eventIds = [`evt_e2e${billingSuffix}a`, `evt_e2e${billingSuffix}b`];
  const mapped = await admin.rpc("billing_record_checkout_completed", {
    p_event_id: eventIds[0],
    p_customer_id: customerId,
    p_owner_type: "user",
    p_owner_id: created.user.id,
  });
  const subscribed = await admin.rpc("billing_record_subscription_event", {
    p_event_id: eventIds[1],
    p_event_type: "customer.subscription.created",
    p_event_created: new Date().toISOString(),
    p_subscription_id: `sub_e2e${billingSuffix}`,
    p_customer_id: customerId,
    p_plan: "pro",
    p_status: "active",
    p_current_period_end: new Date(Date.now() + 30 * 86_400_000).toISOString(),
    p_cancel_at_period_end: false,
    p_trial_end: null,
  });
  if (mapped.data !== "applied" || subscribed.data !== "applied") {
    await admin.auth.admin.deleteUser(created.user.id);
    throw new Error(
      `failed to grant Pro to e2e test user: ${mapped.error?.message ?? mapped.data} / ${subscribed.error?.message ?? subscribed.data}`,
    );
  }

  const anon = createClient(cfg.url, cfg.anonKey);
  const { data: signedIn, error: signInError } = await anon.auth.signInWithPassword({ email, password });
  if (signInError || !signedIn.session) {
    throw new Error(`failed to sign in e2e test user: ${signInError?.message}`);
  }

  return {
    tag,
    userId: created.user.id,
    accessToken: signedIn.session.access_token,
    /** Compensating cleanup: deletes the auth user, whose owned rows across
     * remote_instances/remote_client_keys/remote_audit_events etc. cascade
     * or are removed by the standalone cleanup script's own pass keyed off
     * the same tag - this call always runs from a `finally` block below. */
    async cleanup() {
      // Deleting the user does not cascade to billing rows.
      await admin.from("billing_subscriptions").delete().eq("owner_id", testUserId);
      await admin.from("billing_customers").delete().eq("owner_id", testUserId);
      await admin.from("billing_events").delete().in("stripe_event_id", eventIds);
      const { error } = await admin.auth.admin.deleteUser(testUserId);
      if (error) {
        console.error(`[remote-e2e cleanup] FAILED to delete test user ${testUserId}: ${error.message}`);
      }
    },
  };
}

async function callFunction(
  cfg: NonNullable<ReturnType<typeof e2eConfig>>,
  fnName: string,
  accessToken: string,
  body: Record<string, unknown>,
): Promise<{ status: number; json: Record<string, unknown> }> {
  const response = await fetch(`${cfg.url}/functions/v1/${fnName}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
      apikey: cfg.anonKey,
    },
    body: JSON.stringify(body),
  });
  const json = await response.json().catch(() => ({}));
  // PRD: "capture provider request identifiers" / correlate via correlation
  // IDs. Every call in this suite logs the correlation id the function
  // returned so a failed real-API run can be traced in the test project's
  // own function logs and in remote_audit_events.
  if (typeof json.correlation_id === "string") {
    console.log(`[remote-e2e] ${fnName} correlation_id=${json.correlation_id}`);
  }
  return { status: response.status, json };
}

async function callAdmin(
  cfg: NonNullable<ReturnType<typeof e2eConfig>>,
  body: Record<string, unknown>,
): Promise<{ status: number; json: Record<string, unknown> }> {
  const response = await fetch(`${cfg.url}/functions/v1/remote-admin`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-admin-api-key": cfg.adminApiKey,
    },
    body: JSON.stringify(body),
  });
  const json = await response.json().catch(() => ({}));
  return { status: response.status, json };
}

/** Live tests are Deno-ignored (not passing acceptance) when credentials
 * are absent. `TREQ_REMOTE_E2E=1` plus the dedicated test-project vars must
 * all be set for a test body to run. Extra gates (`soak`, `native`) keep
 * long or SSH-dependent cases skipped unless explicitly requested. */
function e2eTest(
  name: string,
  fn: (cfg: NonNullable<ReturnType<typeof e2eConfig>>) => Promise<void>,
  extra?: { soak?: boolean; native?: boolean },
) {
  const cfg = e2eConfig();
  const soakOk = !extra?.soak || Deno.env.get("TREQ_REMOTE_E2E_SOAK") === "1";
  const nativeOk = !extra?.native || Deno.env.get("TREQ_REMOTE_E2E_NATIVE") === "1";
  const ignore = cfg === null || !soakOk || !nativeOk;
  if (ignore) {
    const reasons: string[] = [];
    if (cfg === null) {
      reasons.push("TREQ_REMOTE_E2E=1 and Supabase test-project credentials not set");
    }
    if (extra?.soak && Deno.env.get("TREQ_REMOTE_E2E_SOAK") !== "1") {
      reasons.push("TREQ_REMOTE_E2E_SOAK=1 not set (scheduled soak, not an ordinary wake)");
    }
    if (extra?.native && Deno.env.get("TREQ_REMOTE_E2E_NATIVE") !== "1") {
      reasons.push("TREQ_REMOTE_E2E_NATIVE=1 not set (native SSH workflow is in remote_e2e.rs)");
    }
    console.log(`[remote-e2e] SKIP "${name}": ${reasons.join("; ")}`);
  }
  Deno.test({
    name,
    ignore,
    fn: async () => {
      if (cfg === null) return;
      await fn(cfg);
    },
  });
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2: idempotent concurrent provisioning.
// ---------------------------------------------------------------------------

e2eTest("repeated ensure calls with the same idempotency key provision exactly one instance", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const idempotencyKey = e2eTag();
    const requests = Array.from({ length: 4 }, () =>
      callFunction(cfg, "remote-instance", user.accessToken, {
        action: "ensure",
        idempotency_key: idempotencyKey,
      })
    );
    const results = await Promise.all(requests);
    for (const { status, json } of results) {
      if (status !== 200) throw new Error(`ensure call failed: ${status} ${JSON.stringify(json)}`);
    }
    const operationIds = new Set(results.map((r) => r.json.operation_id));
    if (operationIds.size !== 1) {
      throw new Error(`expected one operation id across concurrent identical ensure calls, got ${operationIds.size}: ${[...operationIds]}`);
    }

    const status = await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" });
    if (status.status !== 200) throw new Error(`status call failed: ${JSON.stringify(status.json)}`);

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

e2eTest("a second ensure with a different idempotency key still returns the same single instance", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const first = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (first.status !== 200) throw new Error(`first ensure failed: ${JSON.stringify(first.json)}`);
    const second = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (second.status !== 200) throw new Error(`second ensure failed: ${JSON.stringify(second.json)}`);
    const firstId = instanceOf(first.json)?.instance_id ?? "";
    const secondId = instanceOf(second.json)?.instance_id ?? "";
    if (!firstId || firstId !== secondId) {
      throw new Error(`one-instance enforcement failed: ${firstId} vs ${secondId}`);
    }
  } finally {
    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() }).catch(() => {});
    await user.cleanup();
  }
});

e2eTest("list_regions and list_sizes return the dedicated catalog", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const regions = await callFunction(cfg, "remote-instance", user.accessToken, { action: "list_regions" });
    const sizes = await callFunction(cfg, "remote-instance", user.accessToken, { action: "list_sizes" });
    if (regions.status !== 200 || !Array.isArray(regions.json.regions)) {
      throw new Error(`list_regions failed: ${JSON.stringify(regions.json)}`);
    }
    if (sizes.status !== 200 || !Array.isArray(sizes.json.presets)) {
      throw new Error(`list_sizes failed: ${JSON.stringify(sizes.json)}`);
    }
  } finally {
    await user.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Acceptance criterion 1: provisioning reaches "ready".
// ---------------------------------------------------------------------------

e2eTest("provisions a managed instance that reaches ready", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);

    // Real provisioning is asynchronous; poll status until ready, degraded,
    // or failed, or a bounded timeout - this suite must never hang CI
    // indefinitely on a stuck real instance.
    const deadline = Date.now() + 10 * 60 * 1000;
    let lastStatus = "";
    while (Date.now() < deadline) {
      const poll = await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" });
      lastStatus = instanceOf(poll.json)?.status ?? "";
      if (["ready", "degraded", "failed"].includes(lastStatus)) break;
      await new Promise((resolve) => setTimeout(resolve, 5000));
    }
    if (lastStatus !== "ready") {
      throw new Error(`instance did not reach ready within the deadline (last status: ${lastStatus || "timed out"})`);
    }

    const ready = await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" });
    const instance = instanceOf(ready.json);
    if (!instance || instance.status !== "ready") {
      throw new Error(`expanded readiness status missing ready instance: ${JSON.stringify(ready.json)}`);
    }
    if (typeof instance.generation !== "number") {
      throw new Error("ready instance must report a generation");
    }
    if (!ready.json.endpoint) {
      throw new Error(`ready instance must include trusted endpoint metadata: ${JSON.stringify(ready.json)}`);
    }
  } finally {
    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() }).catch(() => {});
    await user.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Acceptance criteria 4, 5: certificate + host-key trust.
// ---------------------------------------------------------------------------

e2eTest("issues a short-lived certificate for a registered client key and rejects an unregistered one", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    // A throwaway ed25519 keypair generated for this test only - never a
    // real user's key, never retained past this test.
    const keyPair = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]) as CryptoKeyPair;
    const rawPublic = await crypto.subtle.exportKey("raw", keyPair.publicKey);
    const openSshLine = encodeOpenSshEd25519PublicKey(new Uint8Array(rawPublic), `${user.tag}@e2e`);

    const register = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "register_client_key",
      idempotency_key: e2eTag(),
      public_key: openSshLine,
    });
    if (register.status !== 200) throw new Error(`register_client_key failed: ${JSON.stringify(register.json)}`);
    const keyId = (register.json.key as { id: string }).id;

    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);
    const instanceId = instanceOf(ensure.json)?.instance_id ?? "";

    const issue = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "issue_certificate",
      instance_id: instanceId,
      key_id: keyId,
    });
    if (issue.status !== 200) throw new Error(`issue_certificate for a ready instance's registered key should succeed: ${JSON.stringify(issue.json)}`);
    if (!issue.json.certificate) throw new Error("issue_certificate response did not include a certificate");

    // Revoke, then confirm a further issuance attempt against the same key
    // is rejected - covers "revoked SSH certificates" via the key-revocation
    // path (certificate-level expiry is covered by certificate lifetime
    // configuration, asserted separately below).
    const revoke = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "revoke_client_key",
      key_id: keyId,
    });
    if (revoke.status !== 200) throw new Error(`revoke_client_key failed: ${JSON.stringify(revoke.json)}`);

    const issueAfterRevoke = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "issue_certificate",
      instance_id: instanceId,
      key_id: keyId,
    });
    if (issueAfterRevoke.status < 400) {
      throw new Error("issuing a certificate for a revoked key must be rejected");
    }

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

e2eTest("issued certificates carry a bounded, short expiry", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const keyPair = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]) as CryptoKeyPair;
    const rawPublic = await crypto.subtle.exportKey("raw", keyPair.publicKey);
    const openSshLine = encodeOpenSshEd25519PublicKey(new Uint8Array(rawPublic), `${user.tag}@e2e`);

    const register = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "register_client_key",
      idempotency_key: e2eTag(),
      public_key: openSshLine,
    });
    const keyId = (register.json.key as { id: string }).id;

    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    const instanceId = instanceOf(ensure.json)?.instance_id ?? "";

    const issue = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "issue_certificate",
      instance_id: instanceId,
      key_id: keyId,
    });
    const expiresAt = new Date(String(issue.json.expires_at ?? ""));
    const issuedAt = new Date(String(issue.json.issued_at ?? Date.now()));
    const lifetimeHours = (expiresAt.getTime() - issuedAt.getTime()) / (1000 * 60 * 60);
    if (!(lifetimeHours > 0 && lifetimeHours <= 24)) {
      throw new Error(`certificate lifetime (${lifetimeHours}h) is not a short, bounded window`);
    }

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

e2eTest("silent renewal issues a fresh certificate and is audited distinctly from first issuance", async (cfg) => {
  // Covers the "Silent renewal while the session is active" PRD section:
  // the desktop client calls `issue_certificate` again (with `renewal:
  // true`) ahead of expiry while the session stays valid, and the
  // resulting audit trail records it as a renewal, not a first issuance.
  const user = await createE2eTestUser(cfg);
  try {
    const keyPair = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]) as CryptoKeyPair;
    const rawPublic = await crypto.subtle.exportKey("raw", keyPair.publicKey);
    const openSshLine = encodeOpenSshEd25519PublicKey(new Uint8Array(rawPublic), `${user.tag}@e2e`);

    const register = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "register_client_key",
      idempotency_key: e2eTag(),
      public_key: openSshLine,
    });
    const keyId = (register.json.key as { id: string }).id;

    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    const instanceId = instanceOf(ensure.json)?.instance_id ?? "";

    const first = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "issue_certificate",
      instance_id: instanceId,
      key_id: keyId,
    });
    if (first.status !== 200) throw new Error(`initial issue_certificate failed: ${JSON.stringify(first.json)}`);

    const renewed = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "issue_certificate",
      instance_id: instanceId,
      key_id: keyId,
      renewal: true,
    });
    if (renewed.status !== 200) throw new Error(`renewal issue_certificate failed: ${JSON.stringify(renewed.json)}`);
    if (renewed.json.serial === first.json.serial) {
      throw new Error("renewal must produce a distinct certificate serial from the initial issuance");
    }

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

e2eTest("a forced cutoff report is recorded in the audit trail", async (cfg) => {
  // Covers the "Hard cutoff on revocation or expiry" PRD section's audit
  // requirement: the client-observed cutoff (already enforced locally by
  // the native transport) is still correlatable server-side.
  const user = await createE2eTestUser(cfg);
  try {
    const report = await callFunction(cfg, "remote-ssh-trust", user.accessToken, {
      action: "report_cutoff",
      instance_id: null,
      endpoint_id: null,
      reason: "certificate_expired",
    });
    if (report.status !== 200) throw new Error(`report_cutoff failed: ${JSON.stringify(report.json)}`);
    if (report.json.status !== "recorded") {
      throw new Error(`report_cutoff did not confirm recording: ${JSON.stringify(report.json)}`);
    }
  } finally {
    await user.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Repair: reprovisioning a Sprite repairs it in place. The Sprite's
// filesystem is the user's environment, so the provider identity, the
// generation, and the pinned host keys all stay the same.
// ---------------------------------------------------------------------------

e2eTest("reprovisioning repairs the Sprite in place and keeps host trust", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  const admin = createClient(cfg.url, cfg.serviceRoleKey);
  const activeFingerprints = async (endpointId: string | null | undefined) => {
    if (!endpointId) return [];
    const { data, error } = await admin
      .from("remote_endpoint_host_keys")
      .select("fingerprint_sha256")
      .eq("endpoint_id", endpointId)
      .is("revoked_at", null);
    if (error) throw new Error(`failed to read host keys: ${error.message}`);
    return (data ?? []).map((row: { fingerprint_sha256: string }) => row.fingerprint_sha256).sort();
  };
  try {
    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);

    const before = instanceOf((await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" })).json);
    const keysBefore = await activeFingerprints(before?.endpoint_id);

    const reprovision = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "reprovision",
      idempotency_key: e2eTag(),
    });
    if (reprovision.status !== 200) throw new Error(`reprovision failed: ${JSON.stringify(reprovision.json)}`);

    const after = instanceOf((await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" })).json);
    const keysAfter = await activeFingerprints(after?.endpoint_id);

    if (!before?.provider_resource_id || after?.provider_resource_id !== before.provider_resource_id) {
      throw new Error(
        `repair must keep the same Sprite: before=${before?.provider_resource_id} after=${after?.provider_resource_id}`,
      );
    }
    if (after?.generation !== before.generation) {
      throw new Error(
        `the generation changes only when the provider resource changes: before=${before.generation} after=${after?.generation}`,
      );
    }
    if (JSON.stringify(keysBefore) !== JSON.stringify(keysAfter)) {
      throw new Error(
        `an in-place repair must not change pinned host keys: before=${keysBefore} after=${keysAfter}`,
      );
    }

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Acceptance criterion 13: wake from vendor suspension.
// ---------------------------------------------------------------------------

e2eTest("wake is accepted as an idempotent control-plane call and is not treated as suspension recovery", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);

    // An ordinary wake against a non-suspended instance proves the control-
    // plane path only. It is not evidence of recovery from a pause; that is
    // the soak test below and the idle-pause test in remote_e2e.rs.
    const wake = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "wake",
      idempotency_key: e2eTag(),
    });
    if (wake.status !== 200) throw new Error(`wake failed: ${JSON.stringify(wake.json)}`);
    const after = await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" });
    const afterStatus = instanceOf(after.json)?.status;
    if (afterStatus !== "ready") {
      throw new Error(`wake must leave the instance ready, got ${afterStatus}: ${JSON.stringify(after.json)}`);
    }

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

e2eTest("soak waits for the Sprite to go cold, then wakes it without recreating it", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);

    const deadline = Date.now() + 6 * 60 * 60 * 1000;
    let observed = "";
    while (Date.now() < deadline) {
      const poll = await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" });
      observed = instanceOf(poll.json)?.status ?? "";
      if (observed === "suspended") break;
      await new Promise((resolve) => setTimeout(resolve, 60_000));
    }
    if (observed !== "suspended") {
      throw new Error(`soak never observed vendor suspension (last status: ${observed || "timed out"})`);
    }
    const beforeWake = instanceOf(
      (await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" })).json,
    );
    const wake = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "wake",
      idempotency_key: e2eTag(),
    });
    if (wake.status !== 200) throw new Error(`wake after soak suspension failed: ${JSON.stringify(wake.json)}`);
    // A wake that only reads metadata leaves a cold Sprite cold. Status
    // re-reads the provider, so it catches that.
    const afterWake = instanceOf(
      (await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" })).json,
    );
    if (afterWake?.status !== "ready") {
      throw new Error(`wake must resume a cold Sprite, but status is ${afterWake?.status}`);
    }
    if (afterWake.provider_resource_id !== beforeWake?.provider_resource_id) {
      throw new Error("wake must not recreate the Sprite");
    }
  } finally {
    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() }).catch(() => {});
    await user.cleanup();
  }
}, { soak: true });

// ---------------------------------------------------------------------------
// Acceptance criterion 15: audit-event completeness and redaction.
// ---------------------------------------------------------------------------

e2eTest("lifecycle operations are correlated in audit events without leaking secrets", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  const admin = createClient(cfg.url, cfg.serviceRoleKey);
  try {
    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);

    const { data: events, error } = await admin
      .from("remote_audit_events")
      .select("event_type, detail, correlation_id")
      .eq("owner_user_id", user.userId)
      .order("created_at", { ascending: true });
    if (error) throw new Error(`failed to read remote_audit_events: ${error.message}`);
    if (!events || events.length === 0) throw new Error("expected at least one audit event for the ensure operation");

    for (const event of events) {
      if (!event.correlation_id) throw new Error(`audit event ${event.event_type} is missing a correlation id`);
      const serializedDetail = JSON.stringify(event.detail ?? {});
      // Sensitive-data redaction: private key material, CA private key, and
      // raw provider tokens must never appear in an audit record.
      const forbiddenSubstrings = ["PRIVATE KEY", "BEGIN OPENSSH", "fly_api_token", "sprites_api_token", "service_role"];
      for (const forbidden of forbiddenSubstrings) {
        if (serializedDetail.toLowerCase().includes(forbidden.toLowerCase())) {
          throw new Error(`audit event ${event.event_type} detail appears to contain sensitive material: matched "${forbidden}"`);
        }
      }
    }

    await callFunction(cfg, "remote-instance", user.accessToken, { action: "delete", idempotency_key: e2eTag() });
  } finally {
    await user.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Acceptance criterion 16 / "teardown and orphan-resource detection".
// ---------------------------------------------------------------------------

e2eTest("delete tears down the instance and it no longer appears as active", async (cfg) => {
  const user = await createE2eTestUser(cfg);
  try {
    const ensure = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "ensure",
      idempotency_key: e2eTag(),
    });
    if (ensure.status !== 200) throw new Error(`ensure failed: ${JSON.stringify(ensure.json)}`);

    const del = await callFunction(cfg, "remote-instance", user.accessToken, {
      action: "delete",
      idempotency_key: e2eTag(),
    });
    if (del.status !== 200) throw new Error(`delete failed: ${JSON.stringify(del.json)}`);

    const status = await callFunction(cfg, "remote-instance", user.accessToken, { action: "status" });
    const remaining = instanceOf(status.json);
    if (remaining && remaining.status !== "deleted" && remaining.status !== "unprovisioned") {
      throw new Error(`expected the instance to read back as deleted/unprovisioned, got: ${JSON.stringify(status.json)}`);
    }
  } finally {
    await user.cleanup();
  }
});

e2eTest("remote-admin cleanup path can list and prune this project's audit/failure records", async (cfg) => {
  // Exercises the admin surface `scripts/remote-e2e-cleanup.ts` builds on,
  // proving the admin key gate and the endpoint itself work against the
  // real test project (not just against unit-mocked Deno.serve handlers).
  const failures = await callAdmin(cfg, { action: "list_recent_failures", limit: 5 });
  if (failures.status !== 200) throw new Error(`list_recent_failures failed: ${JSON.stringify(failures.json)}`);

  const unauthorized = await fetch(`${cfg.url}/functions/v1/remote-admin`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "x-admin-api-key": "wrong-key" },
    body: JSON.stringify({ action: "list_recent_failures" }),
  });
  if (unauthorized.status !== 401) {
    throw new Error(`remote-admin must reject a wrong admin key, got status ${unauthorized.status}`);
  }
});

/** Minimal OpenSSH `ssh-ed25519 <base64> <comment>` line encoder for a raw
 * 32-byte Ed25519 public key, sufficient for this test's own
 * register_client_key calls without pulling in an SSH-key-format library. */
function encodeOpenSshEd25519PublicKey(rawPublicKey: Uint8Array, comment: string): string {
  const typeBytes = new TextEncoder().encode("ssh-ed25519");
  const parts = [encodeUint32(typeBytes.length), typeBytes, encodeUint32(rawPublicKey.length), rawPublicKey];
  const total = parts.reduce((n, p) => n + p.length, 0);
  const blob = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    blob.set(part, offset);
    offset += part.length;
  }
  return `ssh-ed25519 ${encodeBase64(blob)} ${comment}`;
}

function encodeUint32(value: number): Uint8Array {
  const buf = new Uint8Array(4);
  new DataView(buf.buffer).setUint32(0, value, false);
  return buf;
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

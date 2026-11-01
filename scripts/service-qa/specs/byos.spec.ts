/**
 * Bring your own sandbox (BYOS) against the local database: credential
 * actions, Vault, grants and RLS, the placement guard, key removal, and
 * 401 handling (045, 046, 052–054).
 *
 * Runs `remote-instance/lib.ts` in this process with the service-role client
 * and the caller's own signed-in client, wired as remote-instance/index.ts
 * wires them. Providers are the in-memory stub (TREQ_REMOTE_PROVIDER_STUB=1).
 */
import { randomUUID } from "node:crypto";
import type { SupabaseClient } from "@supabase/supabase-js";
import { expect, it } from "vitest";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import {
  providerForPlacement,
  supabaseByosStore,
  supabaseCredentialLoader,
} from "../../../supabase/functions/_shared/remote/byos";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import {
  ProviderError,
  type ProviderKind,
} from "../../../supabase/functions/_shared/remote/provider";
import { providerFor } from "../../../supabase/functions/_shared/remote/provider-registry";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { clearBilling, grantPro } from "../billing";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
  type TestUser,
} from "../seed";

const admin = getServiceClient();
const stubEnv = (k: string) =>
  k === "TREQ_REMOTE_PROVIDER_STUB" ? "1" : undefined;
const build = (kind: ProviderKind, credential?: { apiKey: string }) =>
  providerFor(kind, stubEnv, credential);

type Caller = { id: string; client: SupabaseClient };

async function signIn(u: TestUser): Promise<Caller> {
  const { client } = await signInWithEmailPassword(u.email, u.password);
  return { id: u.user.id, client };
}

async function call(
  caller: Caller,
  action: string,
  body: Record<string, unknown> = {},
) {
  const response = await handleRemoteInstanceAction(
    { action, idempotency_key: `${action}-${randomUUID()}`, ...body },
    {
      supabase: admin,
      ownerUserId: caller.id,
      correlationId: `sqa-${randomUUID()}`,
      hasPro: () => userHasPro(admin, caller.id),
      stub: true,
      provider: (kind) => new LocalProvider(kind),
      byos: {
        canByos: async () =>
          (await admin.rpc("can_byos", { p_user_id: caller.id })).data === true,
        store: supabaseByosStore(admin, caller.client, caller.id),
        build,
      },
    },
  );
  return {
    status: response.status,
    json: (await response.json()) as Record<string, unknown>,
  };
}

async function credentialRow(userId: string, kind: string) {
  const { data, error } = await admin
    .from("remote_provider_credentials")
    .select("id, vault_secret_id, validated_at, invalid_at")
    .eq("owner_user_id", userId)
    .eq("provider_kind", kind)
    .maybeSingle();
  if (error) throw new Error(error.message);
  return data as {
    id: string;
    vault_secret_id: string;
    validated_at: string | null;
    invalid_at: string | null;
  } | null;
}

it("saves a Pro user's key after validation and refuses Free users, Fly, local and bad input", async () => {
  const pro = await createTestUser();
  const free = await createTestUser();
  try {
    await grantPro(admin, pro.user.id);
    const p = await signIn(pro);
    const f = await signIn(free);

    const saved = await call(p, "save_provider_credential", {
      provider_kind: "boat",
      api_key: "sk-sqa-1",
      label: "mine",
    });
    expect(saved.status).toBe(200);
    expect(JSON.stringify(saved.json)).not.toContain("sk-sqa-1");
    expect((await credentialRow(pro.user.id, "boat"))?.invalid_at).toBeNull();

    const refused = {
      free: await call(f, "save_provider_credential", {
        provider_kind: "boat",
        api_key: "k",
      }),
      fly: await call(p, "save_provider_credential", {
        provider_kind: "fly_machines",
        api_key: "k",
      }),
      local: await call(p, "save_provider_credential", {
        provider_kind: "local",
        api_key: "k",
      }),
      unknown: await call(p, "save_provider_credential", {
        provider_kind: "nope",
        api_key: "k",
      }),
      unicode: await call(p, "save_provider_credential", {
        provider_kind: "boat",
        api_key: "ключ",
      }),
      newline: await call(p, "save_provider_credential", {
        provider_kind: "boat",
        api_key: "a\nb",
      }),
      empty: await call(p, "save_provider_credential", {
        provider_kind: "boat",
        api_key: "",
      }),
      huge: await call(p, "save_provider_credential", {
        provider_kind: "boat",
        api_key: "x".repeat(4097),
      }),
      longLabel: await call(p, "save_provider_credential", {
        provider_kind: "boat",
        api_key: "k",
        label: "l".repeat(81),
      }),
    };
    const codes = Object.fromEntries(
      Object.entries(refused).map(([k, r]) => [
        k,
        `${r.status} ${r.json.code}`,
      ]),
    );
    expect(codes).toEqual({
      free: "403 byos_not_allowed",
      fly: "400 byos_kind_unsupported",
      local: "400 byos_kind_unsupported",
      unknown: "400 byos_kind_unsupported",
      unicode: "400 validation_error",
      newline: "400 validation_error",
      empty: "400 validation_error",
      huge: "400 validation_error",
      longLabel: "400 validation_error",
    });
    expect(await credentialRow(free.user.id, "boat")).toBeNull();
    expect(await credentialRow(pro.user.id, "fly_machines")).toBeNull();

    await recordOutcome("byos-01-save-and-refusals", {
      expectations: [
        "A Pro user's validated key is stored and never echoed back.",
        "Free users, fly_machines, local and unknown kinds are refused and store nothing.",
        "Unicode/newline/empty/oversized keys and an 81-char label get 400 validation_error.",
      ],
      details: { saved: saved.json, codes },
    });
  } finally {
    await clearBilling(admin, [pro.user.id, free.user.id]);
    await deleteTestUser(admin, pro.user.id);
    await deleteTestUser(admin, free.user.id);
  }
}, 60000);

it("keeps Vault and other users' credentials out of reach of API roles", async () => {
  const a = await createTestUser();
  const b = await createTestUser();
  try {
    await grantPro(admin, a.user.id);
    await grantPro(admin, b.user.id);
    const ca = await signIn(a);
    const cb = await signIn(b);
    expect(
      (
        await call(ca, "save_provider_credential", {
          provider_kind: "boat",
          api_key: "sk-a",
        })
      ).status,
    ).toBe(200);
    const row = await credentialRow(a.user.id, "boat");

    const anon = getAnonClient();
    const denied: Record<string, string | null> = {};
    for (const [name, client] of [
      ["anon", anon],
      ["userB", cb.client],
    ] as const) {
      denied[`${name}.save`] =
        (
          await client.rpc("provider_credential_save", {
            p_user_id: b.user.id,
            p_kind: "boat",
            p_key: "x",
            p_label: null,
          })
        ).error?.code ?? null;
      denied[`${name}.load`] =
        (
          await client.rpc("provider_credential_load_for_placement", {
            p_placement_id: randomUUID(),
          })
        ).error?.code ?? null;
      denied[`${name}.mark`] =
        (
          await client.rpc("provider_credential_mark_invalid", {
            p_credential_id: row!.id,
            p_key: "sk-a",
          })
        ).error?.code ?? null;
      denied[`${name}.can_byos`] =
        (await client.rpc("can_byos", { p_user_id: a.user.id })).error?.code ??
        null;
    }
    for (const [k, v] of Object.entries(denied)) expect(v, k).not.toBeNull();

    const secretCol = await cb.client
      .from("remote_provider_credentials")
      .select("vault_secret_id");
    expect(secretCol.error).not.toBeNull();
    const othersRows = await cb.client
      .from("remote_provider_credentials")
      .select("id")
      .eq("owner_user_id", a.user.id);
    expect(othersRows.data ?? []).toEqual([]);
    const othersList = await cb.client.rpc("list_provider_credentials");
    expect(othersList.data).toEqual([]);
    const vaultRead = await cb.client
      .schema("vault" as never)
      .from("decrypted_secrets")
      .select("*");
    expect(vaultRead.data ?? []).toEqual([]);
    // B deleting "boat" only touches B's own (absent) row.
    expect(
      (await cb.client.rpc("delete_provider_credential", { p_kind: "boat" }))
        .error,
    ).toBeNull();
    expect(await credentialRow(a.user.id, "boat")).not.toBeNull();
    expect((await credentialRow(a.user.id, "boat"))?.invalid_at).toBeNull();

    await recordOutcome("byos-02-isolation", {
      expectations: [
        "anon and another user cannot save, load, mark invalid or query the gate (each RPC errors).",
        "Another user sees neither vault_secret_id, A's rows, nor Vault secrets.",
        "Another user's delete for the same kind leaves A's key untouched.",
      ],
      details: {
        denied,
        secretColError: secretCol.error?.message,
        vaultError: vaultRead.error?.message,
      },
    });
  } finally {
    await clearBilling(admin, [a.user.id, b.user.id]);
    await deleteTestUser(admin, a.user.id);
    await deleteTestUser(admin, b.user.id);
  }
}, 60000);

it("serializes concurrent first saves of one kind", async () => {
  const u = await createTestUser();
  try {
    await grantPro(admin, u.user.id);
    const results = await Promise.all(
      Array.from({ length: 8 }, (_, i) =>
        admin.rpc("provider_credential_save", {
          p_user_id: u.user.id,
          p_kind: "daytona",
          p_key: `k-${i}`,
          p_label: null,
        }),
      ),
    );
    const errors = results.map((r) => r.error?.message ?? null).filter(Boolean);
    expect(errors).toEqual([]);
    const { data: rows } = await admin
      .from("remote_provider_credentials")
      .select("id")
      .eq("owner_user_id", u.user.id);
    expect(rows).toHaveLength(1);

    await recordOutcome("byos-03-concurrent-save", {
      expectations: [
        "8 simultaneous first saves of one kind all succeed and leave exactly one credential row.",
      ],
      details: { errors, rows },
    });
  } finally {
    await clearBilling(admin, [u.user.id]);
    await deleteTestUser(admin, u.user.id);
  }
}, 60000);

it("runs a BYOS workspace on the owner's key and keeps the key while a machine lives", async () => {
  const u = await createTestUser();
  try {
    await grantPro(admin, u.user.id);
    const c = await signIn(u);
    const noKey = await call(c, "ensure", {
      credential_source: "byos",
      provider_kind: "e2b",
    });
    expect(noKey.json.code).toBe("byos_credential_missing");

    expect(
      (
        await call(c, "save_provider_credential", {
          provider_kind: "boat",
          api_key: "sk-run",
        })
      ).status,
    ).toBe(200);
    const ensured = await call(c, "ensure", {
      credential_source: "byos",
      provider_kind: "boat",
      region: "eu_west",
    });
    expect(ensured.status, JSON.stringify(ensured.json)).toBe(200);
    const { data: placement } = await admin
      .from("remote_instance_placements")
      .select(
        "id, instance_id, credential_source, credential_id, provider_kind, role",
      )
      .eq("owner_user_id", u.user.id)
      .eq("role", "active")
      .single();
    expect(placement).toMatchObject({
      credential_source: "byos",
      provider_kind: "boat",
    });

    const loaded = await admin.rpc("provider_credential_load_for_placement", {
      p_placement_id: placement!.id,
    });
    expect(loaded.data).toMatchObject({
      credential_source: "byos",
      key: "sk-run",
    });

    // Managed switch of an active BYOS placement is refused by the guard.
    const toManaged = await admin
      .from("remote_instance_placements")
      .update({ credential_source: "managed", credential_id: null })
      .eq("id", placement!.id);
    expect(toManaged.error?.message).toContain("byos_placement_must_stay_byos");

    const inUse = await call(c, "delete_provider_credential", {
      provider_kind: "boat",
    });
    expect(inUse).toMatchObject({
      status: 409,
      json: { code: "credential_in_use" },
    });

    // A claimed retire whose destroy failed: dead, machine still up.
    await admin
      .from("remote_instances")
      .update({ active_placement_id: null })
      .eq("id", placement!.instance_id);
    await admin
      .from("remote_instance_placements")
      .update({
        role: "dead",
        provider_resource_id: "boat-orphan",
        state: "stopped",
      })
      .eq("id", placement!.id);
    const orphan = await call(c, "delete_provider_credential", {
      provider_kind: "boat",
    });
    expect(orphan).toMatchObject({
      status: 409,
      json: { code: "credential_in_use" },
    });

    await admin
      .from("remote_instance_placements")
      .update({ state: "deleted" })
      .eq("id", placement!.id);
    const removed = await call(c, "delete_provider_credential", {
      provider_kind: "boat",
    });
    expect(removed.status).toBe(200);
    expect(await credentialRow(u.user.id, "boat")).toBeNull();
    const after = await admin
      .from("remote_instance_placements")
      .select("credential_id")
      .eq("id", placement!.id)
      .single();
    expect(after.data?.credential_id).toBeNull();

    await recordOutcome("byos-04-placement-and-removal", {
      expectations: [
        "BYOS ensure creates an active byos placement that loads the owner's key; switching it to managed is refused.",
        "Removing the key is refused (409 credential_in_use) while the placement is active and while dead with its machine up.",
        "Once the machine is deleted the key is removed and the placement's credential_id becomes null.",
      ],
      details: {
        ensured: ensured.json,
        placement,
        inUse: inUse.json,
        orphan: orphan.json,
      },
    });
  } finally {
    await admin
      .from("remote_instances")
      .delete()
      .eq("owner_user_id", u.user.id);
    await clearBilling(admin, [u.user.id]);
    await deleteTestUser(admin, u.user.id);
  }
}, 90000);

it("marks a key invalid on 401 unless it was rotated meanwhile, and never falls back to treq", async () => {
  const u = await createTestUser();
  try {
    await grantPro(admin, u.user.id);
    const c = await signIn(u);
    await call(c, "save_provider_credential", {
      provider_kind: "daytona",
      api_key: "k-old",
    });
    expect(
      (
        await call(c, "ensure", {
          credential_source: "byos",
          provider_kind: "daytona",
        })
      ).status,
    ).toBe(200);
    const { data: placement } = await admin
      .from("remote_instance_placements")
      .select("id")
      .eq("owner_user_id", u.user.id)
      .eq("role", "active")
      .single();

    const built: Array<string | null> = [];
    let rotateFirst = false;
    const failing = (kind: ProviderKind, cred?: { apiKey: string }) => {
      built.push(cred?.apiKey ?? null);
      const p = new LocalProvider(kind);
      p.getInstance = async () => {
        if (rotateFirst) {
          await admin.rpc("provider_credential_save", {
            p_user_id: u.user.id,
            p_kind: "daytona",
            p_key: "k-new",
            p_label: null,
          });
        }
        throw new ProviderError("unauthorized", "401");
      };
      return p;
    };
    const loader = supabaseCredentialLoader(admin);

    rotateFirst = true;
    await expect(
      providerForPlacement(
        loader,
        "daytona",
        placement!.id,
        failing,
      ).getInstance("x"),
    ).rejects.toThrow();
    const afterRotation = await credentialRow(u.user.id, "daytona");
    expect(afterRotation?.invalid_at).toBeNull();

    rotateFirst = false;
    await expect(
      providerForPlacement(
        loader,
        "daytona",
        placement!.id,
        failing,
      ).getInstance("x"),
    ).rejects.toThrow();
    const afterRevoke = await credentialRow(u.user.id, "daytona");
    expect(afterRevoke?.invalid_at).not.toBeNull();
    expect(built).toEqual(["k-old", "k-new"]);

    const resized = await call(c, "ensure", {
      credential_source: "byos",
      provider_kind: "daytona",
    });
    const listed = await call(c, "list_provider_credentials");
    const resaved = await call(c, "save_provider_credential", {
      provider_kind: "daytona",
      api_key: "k-fixed",
    });
    expect(resaved.status).toBe(200);
    expect((await credentialRow(u.user.id, "daytona"))?.invalid_at).toBeNull();

    await recordOutcome("byos-05-revoked-and-rotated", {
      expectations: [
        "A 401 for a key replaced mid-request leaves the new key valid; a 401 for the current key marks it invalid.",
        "Every provider built for the BYOS placement used the owner's key, never treq's (no null credential).",
        "Re-saving a valid key clears invalid_at.",
      ],
      details: {
        built,
        afterRotation,
        afterRevoke,
        resized: resized.json,
        listed: listed.json,
      },
    });
  } finally {
    await admin
      .from("remote_instances")
      .delete()
      .eq("owner_user_id", u.user.id);
    await clearBilling(admin, [u.user.id]);
    await deleteTestUser(admin, u.user.id);
  }
}, 90000);

// What the deployed functions see of the SSH CA (remote-admin `ca_status`),
// for the verify script after a CA roll: the signing and trusted
// fingerprints, whether the seed matches its public key, and the running
// machines whose last recorded trust is not the trusted set. A stopped
// machine gets the trusted set when it wakes, so only running ones count.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { must } from "./db.ts";
import { caFingerprint, caKeyMaterialFromEnv, caKeyPairMatches, trustedCaKeysFromEnv } from "./ssh-cert.ts";
import { errorMessage } from "../error-message.ts";

export interface MachineTrust {
  instanceId: string;
  /** From its latest `ca_trust_installed`; null when that failed or none is recorded. */
  fingerprints: string[] | null;
}

export interface CaStatusStore {
  runningMachineTrust(): Promise<MachineTrust[]>;
}

export type CaStatus = { ca_error: string } | {
  signing_fingerprint: string;
  trusted_fingerprints: string[];
  seed_matches_public: boolean;
  running_machines: number;
  stale_machines: Array<{ instance_id: string; fingerprints: string[] | null }>;
};

export async function caStatus(store: CaStatusStore): Promise<CaStatus> {
  let ca, trusted;
  try {
    ca = caKeyMaterialFromEnv();
    trusted = await Promise.all(trustedCaKeysFromEnv().map(caFingerprint));
  } catch (err) {
    // These name the secrets, never their values.
    return { ca_error: errorMessage(err) };
  }
  const machines = await store.runningMachineTrust();
  const matches = (fps: string[] | null) =>
    fps !== null && fps.length === trusted.length && trusted.every((f) => fps.includes(f));
  return {
    signing_fingerprint: await caFingerprint(ca.publicKey),
    trusted_fingerprints: trusted,
    seed_matches_public: await caKeyPairMatches(ca),
    running_machines: machines.length,
    stale_machines: machines
      .filter((m) => !matches(m.fingerprints))
      .map((m) => ({ instance_id: m.instanceId, fingerprints: m.fingerprints })),
  };
}

export function supabaseCaStatusStore(supabase: SupabaseClient): CaStatusStore {
  return {
    async runningMachineTrust() {
      const instances = must(
        await supabase.from("remote_instances").select("id").eq("status", "ready"),
        "read running instances",
      ) ?? [];
      if (instances.length === 0) return [];
      const events = must(
        await supabase
          .from("remote_audit_events")
          .select("instance_id, event_type, detail")
          .in("instance_id", instances.map((i) => i.id))
          .in("event_type", ["ca_trust_installed", "ca_trust_install_failed"])
          .order("created_at", { ascending: false }),
        "read ca trust events",
      ) ?? [];
      return instances.map((i) => {
        const latest = events.find((e) => e.instance_id === i.id);
        const fps = latest?.event_type === "ca_trust_installed" ? latest.detail?.ca_fingerprints : null;
        return { instanceId: i.id, fingerprints: Array.isArray(fps) ? fps : null };
      });
    },
  };
}

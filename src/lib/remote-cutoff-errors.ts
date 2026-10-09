import { useRemoteCutoffStore } from "../stores/remoteCutoffStore";
import type { CutoffReason } from "./remote-cert-lifecycle";

/** Maps the Rust `CutoffReason` display text carried in a
 * `credential_cut_off: endpoint <id> (<reason>)` error to its reason. */
const CUTOFF_REASON_TEXT: [string, CutoffReason][] = [
  ["(session ended)", "session_ended"],
  ["(client key revoked)", "key_revoked"],
  ["(instance no longer accessible)", "instance_inaccessible"],
];

function cutoffReasonFromError(error: unknown): CutoffReason | null {
  const message = error instanceof Error ? error.message : String(error);
  if (
    !message.includes("credential_cut_off") &&
    !message.includes("CredentialCutOff")
  ) {
    return null;
  }
  const match = CUTOFF_REASON_TEXT.find(([text]) => message.includes(text));
  return match ? match[1] : "certificate_expired";
}

export function noteCutoffFromError(error: unknown, endpointId: string | null) {
  if (!endpointId) return;
  const reason = cutoffReasonFromError(error);
  if (reason) useRemoteCutoffStore.getState().recordCutoff(endpointId, reason);
}

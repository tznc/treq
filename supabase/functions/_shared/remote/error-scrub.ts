// Error text that is stored where the owner can read it
// (remote_instance_operations.error_message and remote_audit_events.detail,
// both readable under RLS). A provider's raw message can carry tokens, signed
// URLs or internal hostnames, so what is stored is a code plus the message
// with those parts redacted. Full messages go to the function log only.

import { ProviderError } from "./provider.ts";

const MAX_STORED_LENGTH = 300;

const REDACTIONS: Array<[RegExp, string]> = [
  // URLs, including any query string.
  [/\b[a-z][a-z0-9+.-]*:\/\/\S+/gi, "[url]"],
  // Authorization values.
  [/\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=-]{8,}/gi, "$1 [redacted]"],
  // key=value / key: value pairs whose key names a secret.
  [/\b([\w-]*(?:token|secret|key|password|passwd|auth|signature|sig|credential)[\w-]*)(\s*[=:]\s*)("[^"]*"|'[^']*'|[^\s,;&]+)/gi, "$1$2[redacted]"],
  // Well-known secret prefixes (Stripe, GitHub, Slack, AWS, JWTs).
  [/\b(?:sk|pk|rk)_(?:live|test)_[A-Za-z0-9]+/g, "[redacted]"],
  [/\b(?:gh[pousr]_|github_pat_|xox[abprs]-|AKIA)[A-Za-z0-9_-]{8,}/g, "[redacted]"],
  [/\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g, "[redacted]"],
  // Any other long opaque run (hex or base64-ish).
  [/\b[A-Za-z0-9_+/=-]{32,}\b/g, "[redacted]"],
];

/** The message with URLs and token-like strings replaced, and bounded. */
export function scrubErrorText(text: string): string {
  let out = text;
  for (const [pattern, replacement] of REDACTIONS) out = out.replace(pattern, replacement);
  return out.length > MAX_STORED_LENGTH ? `${out.slice(0, MAX_STORED_LENGTH)}…` : out;
}

/** A stable code for the error: `provider_<kind>`, the error's own `code`, or `internal_error`. */
export function storedErrorCode(err: unknown): string {
  if (err instanceof ProviderError) return `provider_${err.kind}`;
  const code = (err as { code?: unknown } | null)?.code;
  return typeof code === "string" && /^[a-z_]{1,64}$/.test(code) ? code : "internal_error";
}

/** What to store for a failure: its code and the scrubbed message. */
export function storedError(err: unknown): { code: string; message: string } {
  const raw = err instanceof Error ? err.message : String(err);
  return { code: storedErrorCode(err), message: scrubErrorText(raw) };
}

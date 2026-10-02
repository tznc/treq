// Idempotency keys for user-initiated remote mutations
// (prds/remote-development.md, "Structured command protocol" > "Retrying
// after network loss").
//
// The VM uses the key to recognise a repeat of a command it has already
// run. A key built from `Date.now()` or a fresh UUID changes on every tap, so
// when a user taps "Confirm" again after an ambiguous result the VM sees a
// new command and can run it twice. The key must instead belong to the
// user's action: the same inputs, retried before any confirmed outcome,
// reuse the same key. A confirmed outcome ends the action, so the next tap
// starts a new one with a new key.
//
// The store is module-level so a pending key outlives the component that
// sent it: the mobile repo view remounts on resume and reconnect, and the
// desktop adapter has no component at all.

import type { SshEndpoint } from "./api-types-remote";
import {
  dispatchMutationOverSsh,
  type MutationDispatchResult,
  type TreqCommandRequest,
} from "./remote-dispatch";

/** A mutation request whose idempotency key the store fills in. */
export type UnkeyedRequest =
  Extract<TreqCommandRequest, { idempotency_key: string }> extends infer R
    ? R extends unknown
      ? Omit<R, "idempotency_key">
      : never
    : never;

/** JSON with object keys sorted, so property order never changes a fingerprint. */
function canonicalJson(value: unknown): string {
  return JSON.stringify(value, (_key, inner: unknown) =>
    inner && typeof inner === "object" && !Array.isArray(inner)
      ? Object.fromEntries(
          Object.entries(inner).sort(([a], [b]) => (a < b ? -1 : 1)),
        )
      : inner,
  );
}

/**
 * How long an unconfirmed key stays reusable after its last send. A retry of
 * the same uncertain attempt comes within minutes; an identical click after
 * that is a new action, and reusing the key would make the VM replay the old
 * result instead of running it (a push after new commits would not push).
 */
export const PENDING_KEY_TTL_MS = 10 * 60 * 1000;

interface PendingKey {
  key: string;
  scope: string;
  expiresAt: number;
}

export class ActionIdempotencyKeys {
  /** Unconfirmed actions: input fingerprint -> the key they were sent with. */
  private readonly pending = new Map<string, PendingKey>();
  private readonly newId: () => string;
  private readonly now: () => number;

  constructor(
    newId: () => string = () => crypto.randomUUID(),
    now: () => number = Date.now,
  ) {
    this.newId = newId;
    this.now = now;
  }

  /**
   * The key for an action with these inputs. Returns the earlier key while
   * an action with the same inputs has no confirmed outcome and has not
   * expired; otherwise starts a new action. `scope` names the endpoint
   * generation and repository, so a key never crosses to another VM and
   * `release` can end every pending action for one repository.
   */
  keyFor(prefix: string, inputs: readonly unknown[], scope = ""): string {
    const fingerprint = canonicalJson([scope, prefix, ...inputs]);
    const expiresAt = this.now() + PENDING_KEY_TTL_MS;
    const entry = this.pending.get(fingerprint);
    if (entry && entry.expiresAt > this.now()) {
      entry.expiresAt = expiresAt;
      return entry.key;
    }
    const key = `${prefix}:${this.newId()}`;
    this.pending.set(fingerprint, { key, scope, expiresAt });
    return key;
  }

  /**
   * Records the outcome of the dispatch sent with `key`. Only a confirmed
   * outcome ends the action: after an ambiguous result the user may retry,
   * and that retry must carry the same key. A thrown dispatch never reaches
   * here, so its key is kept too: the error may be a dropped connection with
   * an unknown outcome, and reusing a key after a structured failure is
   * harmless because the VM abandons the claim on failure.
   */
  settle(key: string, result: MutationDispatchResult<unknown>): void {
    for (const [fingerprint, entry] of this.pending) {
      if (entry.key !== key) continue;
      if (result.status === "ambiguous") {
        entry.expiresAt = this.now() + PENDING_KEY_TTL_MS;
      } else {
        this.pending.delete(fingerprint);
      }
    }
  }

  /**
   * Ends the pending actions in `scope`, or all of them. Call it only when
   * the user asks for fresh remote state (Refresh): a background change
   * cannot be told apart from the ambiguous attempt itself landing.
   */
  release(scope?: string): void {
    for (const [fingerprint, entry] of this.pending) {
      if (scope === undefined || entry.scope === scope) {
        this.pending.delete(fingerprint);
      }
    }
  }
}

/** The app-wide store shared by the desktop adapter and the mobile screens. */
export const remoteActionKeys = new ActionIdempotencyKeys();

/** The pending-key scope for mobile actions sent to this endpoint generation. */
export function endpointScope(endpoint: SshEndpoint): string {
  const generation =
    endpoint.source.type === "managed" ? endpoint.source.generation : 0;
  return `${endpoint.id}#${generation}`;
}

/**
 * Sends a keyed mutation over SSH with the shared store's key for these
 * inputs on this endpoint generation, so a retry after the screen remounts
 * (resume, reconnect) still reuses the key of the unconfirmed attempt.
 */
export async function dispatchKeyedMutationOverSsh<T = unknown>(
  endpoint: SshEndpoint,
  request: UnkeyedRequest,
): Promise<MutationDispatchResult<T>> {
  const key = remoteActionKeys.keyFor(
    request.kind,
    [request],
    endpointScope(endpoint),
  );
  const result = await dispatchMutationOverSsh<T>(endpoint, {
    ...request,
    idempotency_key: key,
  } as TreqCommandRequest);
  remoteActionKeys.settle(key, result);
  return result;
}

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

export class ActionIdempotencyKeys {
  /** Unconfirmed actions: input fingerprint -> the key they were sent with. */
  private readonly pending = new Map<string, string>();
  private readonly newId: () => string;

  constructor(newId: () => string = () => crypto.randomUUID()) {
    this.newId = newId;
  }

  /**
   * The key for an action with these inputs. Returns the earlier key while
   * an action with the same inputs has no confirmed outcome; otherwise
   * starts a new action. Callers include the endpoint identity and
   * generation in `inputs` so a key never crosses to another VM.
   */
  keyFor(prefix: string, inputs: readonly unknown[]): string {
    const fingerprint = canonicalJson([prefix, ...inputs]);
    let key = this.pending.get(fingerprint);
    if (!key) {
      key = `${prefix}:${this.newId()}`;
      this.pending.set(fingerprint, key);
    }
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
    if (result.status === "ambiguous") return;
    for (const [fingerprint, pendingKey] of this.pending) {
      if (pendingKey === key) this.pending.delete(fingerprint);
    }
  }

  clear(): void {
    this.pending.clear();
  }
}

/** The app-wide store shared by the desktop adapter and the mobile screens. */
export const remoteActionKeys = new ActionIdempotencyKeys();

/**
 * Sends a keyed mutation over SSH with the shared store's key for these
 * inputs on this endpoint generation, so a retry after the screen remounts
 * (resume, reconnect) still reuses the key of the unconfirmed attempt.
 */
export async function dispatchKeyedMutationOverSsh<T = unknown>(
  endpoint: SshEndpoint,
  request: UnkeyedRequest,
): Promise<MutationDispatchResult<T>> {
  const generation =
    endpoint.source.type === "managed" ? endpoint.source.generation : 0;
  const key = remoteActionKeys.keyFor(request.kind, [
    endpoint.id,
    generation,
    request,
  ]);
  const result = await dispatchMutationOverSsh<T>(endpoint, {
    ...request,
    idempotency_key: key,
  } as TreqCommandRequest);
  remoteActionKeys.settle(key, result);
  return result;
}

// Waking a stopped machine, recorded. A wake moves the placement through
// `waking` to `ready` (set_placement_state, so billing follows), and on a
// provider without a service supervisor sshd and the gateway are started
// again (machine-setup.ts `reensureServicesAfterWake`). Every wake goes
// through here: the engine's preflight and verify steps, the resize
// preflight, reverse cutover and the ssh relay.

import type { SandboxProvider } from "./provider.ts";

export interface WakeHooks {
  /** set_placement_state; omitted where no placement is known. */
  setState?: (placementId: string, state: string) => Promise<void>;
  /** Restarts services after the machine is up. */
  afterWake: (provider: SandboxProvider, providerResourceId: string, placementId: string | null) => Promise<void>;
}

export async function wakeRecorded(
  provider: SandboxProvider,
  providerResourceId: string,
  placementId: string | null,
  hooks: WakeHooks,
): Promise<void> {
  if (placementId && hooks.setState) await hooks.setState(placementId, "waking");
  await provider.wakeInstance(providerResourceId);
  await hooks.afterWake(provider, providerResourceId, placementId);
  if (placementId && hooks.setState) await hooks.setState(placementId, "ready");
}

/** Wakes the machine unless it already runs. True when it was woken. */
export async function wakeIfStopped(
  provider: SandboxProvider,
  providerResourceId: string,
  placementId: string | null,
  hooks: WakeHooks,
): Promise<boolean> {
  const observed = await provider.getInstance(providerResourceId);
  if (observed.state === "ready") return false;
  await wakeRecorded(provider, providerResourceId, placementId, hooks);
  return true;
}

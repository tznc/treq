/**
 * A test-only SSH endpoint whose typed commands run through the real local
 * executor (`remote_dispatch_local`) instead of a network connection, so
 * remote UI tests exercise the same typed routes a real endpoint does.
 */
export const LOOPBACK_SSH_HOSTNAME = "treq-test-loopback.invalid";

/** The host-key fingerprint the loopback endpoint "presents" to a probe. */
export const LOOPBACK_SSH_HOST_KEY_FINGERPRINT = "SHA256:loopback";

/** A syntactically valid OpenSSH public key for `~/.ssh` fixtures. */
export const TEST_SSH_PUBLIC_KEY =
  "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOKAQEWDZ1EUztxmGvIoaFl5xy1HcBCOXuH5gXT6XI4O qa@treq";

type Invoke = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

function isLoopback(args: Record<string, unknown>): boolean {
  const endpoint = args.endpoint as { hostname?: string } | undefined;
  return endpoint?.hostname === LOOPBACK_SSH_HOSTNAME;
}

/** Routes loopback-endpoint dispatches locally; null for every other call. */
export function routeLoopbackSsh(
  invoke: Invoke,
  cmd: string,
  args: Record<string, unknown>,
): Promise<unknown> | null {
  if (cmd === "probe_ssh_host_key") {
    return args.hostname === LOOPBACK_SSH_HOSTNAME
      ? Promise.resolve(LOOPBACK_SSH_HOST_KEY_FINGERPRINT)
      : null;
  }
  if (!isLoopback(args)) return null;
  const local = () =>
    invoke("remote_dispatch_local", { request: args.request });
  if (cmd === "remote_dispatch_over_ssh") return local();
  if (cmd === "remote_dispatch_mutation_over_ssh") {
    return local().then((value) => ({ status: "applied", value }));
  }
  return null;
}

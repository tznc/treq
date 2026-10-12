import { invoke } from "@tauri-apps/api/core";
import type { ResolvedSshAlias } from "./api-types";

/**
 * Resolves an explicitly-selected `~/.ssh/config` alias into its hostname,
 * port, username, and identity fields. Autocomplete only: this performs no
 * network I/O and never grants trust by itself. The user still supplies and
 * confirms the expected host-key fingerprint before the endpoint is
 * registered.
 */
export const resolveSshConfigAlias = (
  alias: string,
): Promise<ResolvedSshAlias> => invoke("resolve_ssh_config_alias", { alias });

/**
 * Reads the host-key fingerprint `hostname:port` presents, without
 * authenticating, so the user can confirm it before Treq pins it.
 */
export const probeSshHostKey = (
  hostname: string,
  port: number,
): Promise<string> => invoke("probe_ssh_host_key", { hostname, port });

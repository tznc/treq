// A placement's integration credential: a secret its sandbox signs
// integration-proxy requests with, so agents get GitHub tokens without a
// user in the loop. Saved to Vault (078_integration_secrets.sql) before it
// is written to the machine, rotated on every provision, and gone with the
// placement. Unlike the gateway secret it is readable by the sandbox's
// `treq` user, whom agents run as.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { must } from "./db.ts";
import { generateGatewaySecret } from "./gateway-token.ts";
import { ProviderError, type SandboxProvider } from "./provider.ts";

export const INTEGRATION_CONFIG_FILE = "/etc/treq/integration.json";
export const INTEGRATION_CONFIG_ENV = "TREQ_INTEGRATION_CONFIG";

export interface IntegrationSecretStore {
  save(placementId: string, secret: string): Promise<void>;
}

export function supabaseIntegrationSecrets(supabase: SupabaseClient): IntegrationSecretStore {
  return {
    async save(placementId, secret) {
      must(
        await supabase.rpc("integration_secret_save", { p_placement_id: placementId, p_secret: secret }),
        "save integration secret",
      );
    },
  };
}

/** integration-proxy's URL on this project. */
export function integrationProxyUrl(): string {
  return `${Deno.env.get("SUPABASE_URL") ?? ""}/functions/v1/integration-proxy`;
}

// Writes the config ({ placement_id, proxy_url, secret }) mode 640, group
// treq when that user exists. It arrives in the exec environment and is
// written by the shell's builtin printf through a pipe, so it is never in
// this script or any argv on the machine (the residual is as for the
// gateway secret, see ssh-vm-config.ts).
export function installIntegrationConfigCommand(): string[] {
  const script = `#!/bin/sh
set -eu
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi
$SUDO mkdir -p /etc/treq
$SUDO install -m 600 /dev/null ${INTEGRATION_CONFIG_FILE}
printf "%s\\n" "$${INTEGRATION_CONFIG_ENV}" | $SUDO tee ${INTEGRATION_CONFIG_FILE} >/dev/null
if getent group treq >/dev/null 2>&1; then $SUDO chgrp treq ${INTEGRATION_CONFIG_FILE}; fi
$SUDO chmod 640 ${INTEGRATION_CONFIG_FILE}
echo "treq: integration credential installed"
`;
  return ["/bin/sh", "-c", script];
}

export async function provisionIntegrationCredential(
  secrets: IntegrationSecretStore,
  provider: SandboxProvider,
  providerResourceId: string,
  placementId: string,
  proxyUrl: string,
): Promise<void> {
  const secret = generateGatewaySecret();
  await secrets.save(placementId, secret);
  const config = JSON.stringify({ placement_id: placementId, proxy_url: proxyUrl, secret });
  const out = await provider.execOnMachine(
    providerResourceId,
    installIntegrationConfigCommand(),
    undefined,
    { [INTEGRATION_CONFIG_ENV]: config },
  );
  if (out.exitCode !== 0) {
    throw new ProviderError(
      "other",
      `integration credential install exited ${out.exitCode}: ${out.stderr || out.stdout}`,
    );
  }
  const helpers = await provider.execOnMachine(providerResourceId, configureGithubHelpersCommand());
  if (helpers.exitCode !== 0) {
    throw new ProviderError(
      "other",
      `GitHub helper setup exited ${helpers.exitCode}: ${helpers.stderr || helpers.stdout}`,
    );
  }
}

// Points git (system-wide, for https://github.com, per repository path) and
// gh at `treq github`, which fetches installation tokens with the
// integration credential. gh is shimmed only when it is installed; the shim
// in /usr/local/bin runs the real /usr/bin/gh through treq. Needs a treq
// release that has `treq github`.
export function configureGithubHelpersCommand(): string[] {
  const script = `#!/bin/sh
set -eu
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi
$SUDO git config --system credential.https://github.com.helper "/usr/local/bin/treq github credential"
$SUDO git config --system credential.https://github.com.useHttpPath true
if [ -x /usr/bin/gh ]; then
  printf '%s\\n' '#!/bin/sh' 'exec /usr/local/bin/treq github gh -- "$@"' | $SUDO tee /usr/local/bin/gh >/dev/null
  $SUDO chmod 755 /usr/local/bin/gh
fi
echo "treq: GitHub helpers configured"
`;
  return ["/bin/sh", "-c", script];
}

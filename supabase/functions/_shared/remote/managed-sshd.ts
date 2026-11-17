// Brings up sshd on a managed machine and reads its real host key.
//
// A managed machine has no raw TCP ingress, so sshd only listens on
// loopback inside it and clients reach it through the `remote-ssh-relay`
// Edge Function, which opens the machine's treq-gateway through the
// provider's ingress. Provider tokens never reach the client. SSH itself stays end to
// end: the client still verifies the host key recorded here and presents a
// Treq CA certificate that this sshd checks.

import { parseOpenSshPublicKey } from "./ssh-keys.ts";
import type { ManagedComputeProvider } from "./provider.ts";
import {
  installCaTrustCommand,
  installSshdCommand,
  MANAGED_SSHD_LAUNCHER,
  MANAGED_SSHD_SERVICE_NAME,
  readHostKeysCommand,
} from "./ssh-vm-config.ts";

export interface ManagedHostKey {
  algorithm: string;
  fingerprintSha256: string;
}

async function execOrThrow(
  provider: ManagedComputeProvider,
  providerId: string,
  command: string[],
  stage: string,
  timeoutSeconds?: number,
): Promise<string> {
  const result = await provider.execOnMachine(
    providerId,
    command,
    timeoutSeconds,
  );
  if (result.exitCode !== 0) {
    throw new Error(
      `${stage} exited ${result.exitCode}: ${result.stderr || result.stdout}`,
    );
  }
  return result.stdout;
}

// Installs sshd, trusts the CA, and (re)starts the sshd service. Order
// matters: the launcher points sshd at the CA file, so the CA must be on
// disk before the service restarts.
export async function provisionManagedSshd(
  provider: ManagedComputeProvider,
  providerId: string,
  caPublicKeyLine: string,
): Promise<void> {
  await execOrThrow(
    provider,
    providerId,
    installSshdCommand(),
    "sshd install",
    300,
  );
  await execOrThrow(
    provider,
    providerId,
    installCaTrustCommand(caPublicKeyLine),
    "ca trust install",
  );
  await provider.ensureService(providerId, MANAGED_SSHD_SERVICE_NAME, {
    cmd: MANAGED_SSHD_LAUNCHER,
    args: [],
  });
}

// Reads the ed25519 host key over the provider's authenticated exec API.
// Returns an empty list when the output holds no parsable key, so the
// caller records a readiness failure instead of a made-up fingerprint.
export async function readManagedHostKeys(
  provider: ManagedComputeProvider,
  providerId: string,
): Promise<ManagedHostKey[]> {
  const stdout = await execOrThrow(
    provider,
    providerId,
    readHostKeysCommand(),
    "host key read",
  );
  return await parseHostKeyLines(stdout);
}

export async function parseHostKeyLines(
  text: string,
): Promise<ManagedHostKey[]> {
  const keys: ManagedHostKey[] = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    try {
      const parsed = await parseOpenSshPublicKey(line);
      keys.push({
        algorithm: parsed.algorithm,
        fingerprintSha256: parsed.fingerprintSha256,
      });
    } catch {
      // Not a key line (for example a shell warning); skip it.
    }
  }
  return keys;
}

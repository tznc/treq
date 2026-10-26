// Shell command generators for configuring SSH trust on an already-booted
// managed VM. These are run
// through `ManagedComputeProvider.execOnMachine`, not interpolated from
// frontend text (PRD "Do not interpolate frontend text into remote shell
// scripts") - every value here is server-generated: a CA public key line or
// an already-validated OpenSSH public key line.

const MANAGED_SSH_USER_HOME = "/home/treq";
const TRUSTED_CA_MARKER_FILE = "/etc/ssh/treq_ca.pub";
const SSHD_CONFIG_DROPIN = "/etc/ssh/sshd_config.d/60-treq-ca.conf";

// Idempotent: writes the CA public key line to a fixed path and points
// `TrustedUserCAKeys` at it via an sshd config drop-in, then reloads sshd.
// Safe to re-run (e.g. on every reprovision) since it only overwrites its own
// two files. Sprite exec may run as a non-root user with passwordless sudo,
// so writes under /etc go through `$SUDO`.
export function installCaTrustCommand(caPublicKeyLine: string): string[] {
  const script = `#!/bin/sh
set -eu
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi
$SUDO mkdir -p /etc/ssh/sshd_config.d
$SUDO tee "${TRUSTED_CA_MARKER_FILE}" >/dev/null <<'TREQ_CA_EOF'
${caPublicKeyLine}
TREQ_CA_EOF
$SUDO tee "${SSHD_CONFIG_DROPIN}" >/dev/null <<'TREQ_SSHD_EOF'
TrustedUserCAKeys ${TRUSTED_CA_MARKER_FILE}
TREQ_SSHD_EOF
if command -v systemctl >/dev/null 2>&1 && systemctl is-active sshd >/dev/null 2>&1; then
  $SUDO systemctl reload sshd
elif command -v systemctl >/dev/null 2>&1 && systemctl is-active ssh >/dev/null 2>&1; then
  $SUDO systemctl reload ssh
elif [ -f /var/run/sshd.pid ]; then
  $SUDO kill -HUP "$(cat /var/run/sshd.pid)"
fi
echo "treq: CA trust installed"
`;
  return ["/bin/sh", "-c", script];
}

// sshd on a Sprite. Sprites have no raw TCP ingress (the public URL is
// HTTP-only), so sshd listens on loopback only and is reached through the
// Sprites TCP proxy by the `remote-ssh-relay` Edge Function. Port 2222
// avoids clashing with any sshd the base image might already run.
export const MANAGED_SSHD_HOST = "localhost";
export const MANAGED_SSHD_PORT = 2222;
export const MANAGED_SSHD_SERVICE_NAME = "treq-sshd";
export const MANAGED_SSHD_LAUNCHER = "/usr/local/bin/treq-sshd";
const MANAGED_SSH_USER = "treq";

// Idempotent sshd install for a Sprite: installs openssh-server when absent,
// creates the login user, generates any missing host keys, and writes the
// launcher script the Sprites service runs. The launcher passes every
// security-relevant option on the command line so a base-image
// sshd_config cannot widen access: loopback only, no passwords, and user
// certificates signed by the Treq CA (written by `installCaTrustCommand`).
export function installSshdCommand(): string[] {
  const script = `#!/bin/sh
set -eu
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi
# Persistent terminals need nothing here: the treq CLI's own daemon holds
# them (pty_daemon.rs), so a terminal survives a relay drop and reattaches.
missing=""
[ -x /usr/sbin/sshd ] || missing="$missing openssh-server"
if [ -n "$missing" ]; then
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get update -q
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -q $missing
fi
if ! id -u ${MANAGED_SSH_USER} >/dev/null 2>&1; then
  $SUDO useradd --create-home --shell /bin/bash ${MANAGED_SSH_USER}
fi
# useradd leaves the account locked ("!"), which sshd refuses even for
# certificate logins. "*" means no password without locking the account.
$SUDO usermod -p '*' ${MANAGED_SSH_USER}
$SUDO mkdir -p /run/sshd
$SUDO ssh-keygen -A
$SUDO tee "${MANAGED_SSHD_LAUNCHER}" >/dev/null <<'TREQ_SSHD_LAUNCHER_EOF'
#!/bin/sh
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi
$SUDO mkdir -p /run/sshd
exec $SUDO /usr/sbin/sshd -D -e \
  -o ListenAddress=127.0.0.1 \
  -o Port=${MANAGED_SSHD_PORT} \
  -o PasswordAuthentication=no \
  -o KbdInteractiveAuthentication=no \
  -o PermitRootLogin=no \
  -o TrustedUserCAKeys=${TRUSTED_CA_MARKER_FILE} \
  -o PidFile=/run/treq-sshd.pid
TREQ_SSHD_LAUNCHER_EOF
$SUDO chmod 755 "${MANAGED_SSHD_LAUNCHER}"
echo "treq: sshd installed"
`;
  return ["/bin/sh", "-c", script];
}

// Prints the sshd host public keys. Read over the provider's authenticated
// exec API rather than scanned over the network, so the recorded
// fingerprint comes from the machine itself instead of trust on first use.
export function readHostKeysCommand(): string[] {
  return ["/bin/sh", "-c", "cat /etc/ssh/ssh_host_ed25519_key.pub"];
}

// Idempotent authorized_keys install: appends the key only if a line with
// the same fingerprint marker comment is not already present, so repeated
// installs (retry, re-registration) never duplicate an entry.
export function installAuthorizedKeyCommand(publicKeyLine: string, fingerprintSha256: string): string[] {
  const marker = `# treq-client-key:${fingerprintSha256}`;
  const script = `#!/bin/sh
set -eu
mkdir -p "${MANAGED_SSH_USER_HOME}/.ssh"
chmod 700 "${MANAGED_SSH_USER_HOME}/.ssh"
touch "${MANAGED_SSH_USER_HOME}/.ssh/authorized_keys"
if ! grep -qF "${marker}" "${MANAGED_SSH_USER_HOME}/.ssh/authorized_keys" 2>/dev/null; then
  printf '%s\\n%s\\n' "${marker}" "${publicKeyLine}" >> "${MANAGED_SSH_USER_HOME}/.ssh/authorized_keys"
fi
chmod 600 "${MANAGED_SSH_USER_HOME}/.ssh/authorized_keys"
echo "treq: authorized key installed"
`;
  return ["/bin/sh", "-c", script];
}

// Removes exactly the two lines (marker + key) this module's install added
// for a given fingerprint, leaving every other entry untouched.
export function removeAuthorizedKeyCommand(fingerprintSha256: string): string[] {
  const marker = `# treq-client-key:${fingerprintSha256}`;
  const script = `#!/bin/sh
set -eu
FILE="${MANAGED_SSH_USER_HOME}/.ssh/authorized_keys"
if [ -f "$FILE" ]; then
  MARKER="${marker}"
  awk -v marker="$MARKER" '
    $0 == marker { skip = 2; next }
    skip > 0 { skip--; next }
    { print }
  ' "$FILE" > "$FILE.treq_tmp"
  mv "$FILE.treq_tmp" "$FILE"
fi
echo "treq: authorized key removed"
`;
  return ["/bin/sh", "-c", script];
}

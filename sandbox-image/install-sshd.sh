#!/bin/sh
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
if ! id -u treq >/dev/null 2>&1; then
  $SUDO useradd --create-home --shell /bin/bash treq
fi
# useradd leaves the account locked ("!"), which sshd refuses even for
# certificate logins. "*" means no password without locking the account.
$SUDO usermod -p '*' treq
$SUDO mkdir -p /run/sshd
$SUDO ssh-keygen -A
$SUDO tee "/usr/local/bin/treq-sshd" >/dev/null <<'TREQ_SSHD_LAUNCHER_EOF'
#!/bin/sh
SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo -n"; fi
$SUDO mkdir -p /run/sshd
exec $SUDO /usr/sbin/sshd -D -e   -o ListenAddress=127.0.0.1   -o Port=2222   -o PasswordAuthentication=no   -o KbdInteractiveAuthentication=no   -o PermitRootLogin=no   -o TrustedUserCAKeys=/etc/ssh/treq_ca.pub   -o PidFile=/run/treq-sshd.pid
TREQ_SSHD_LAUNCHER_EOF
$SUDO chmod 755 "/usr/local/bin/treq-sshd"
echo "treq: sshd installed"

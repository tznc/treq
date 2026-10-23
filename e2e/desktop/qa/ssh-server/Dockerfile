# The SSH host 18-ssh-remote.spec.ts connects to: sshd with key-only login
# for `dev`, plus what Treq needs on a host (git, tmux; `treq` and `jj` are
# copied in by up.sh). Claude Code is not baked in: the spec installs it
# from a Treq terminal with the official installer, as a user would.
FROM ubuntu:24.04
RUN apt-get update \
  && apt-get install -y --no-install-recommends openssh-server git tmux curl ca-certificates \
  && rm -rf /var/lib/apt/lists/*
RUN useradd -m -s /bin/bash dev \
  && mkdir -p /run/sshd /home/dev/.ssh \
  && chown dev:dev /home/dev/.ssh && chmod 700 /home/dev/.ssh \
  && sed -i 's/^#\?PasswordAuthentication .*/PasswordAuthentication no/' /etc/ssh/sshd_config \
  && echo 'export PATH="$HOME/.local/bin:$PATH"' >> /home/dev/.profile
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]

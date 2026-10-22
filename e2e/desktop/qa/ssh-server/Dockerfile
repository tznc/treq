# The SSH host 18-ssh-remote.spec.ts connects to: sshd with key-only login
# for `dev`, plus what Treq needs on a host (git, tmux; `treq` and `jj` are
# copied in by up.sh) and a stand-in `claude` agent.
FROM ubuntu:24.04
RUN apt-get update \
  && apt-get install -y --no-install-recommends openssh-server git tmux ca-certificates \
  && rm -rf /var/lib/apt/lists/*
RUN useradd -m -s /bin/bash dev \
  && mkdir -p /run/sshd /home/dev/.ssh \
  && chown dev:dev /home/dev/.ssh && chmod 700 /home/dev/.ssh \
  && sed -i 's/^#\?PasswordAuthentication .*/PasswordAuthentication no/' /etc/ssh/sshd_config
RUN printf '#!/bin/sh\nprintf "\\033[1m> remote agent ready\\033[0m (args: %%s)\\n" "$*"\nprintf "\\033[32m●\\033[0m Running on $(hostname) in $(pwd)\\n"\nexec sh\n' \
  > /usr/local/bin/claude && chmod +x /usr/local/bin/claude
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]

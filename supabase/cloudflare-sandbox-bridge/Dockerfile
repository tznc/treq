# The Sandbox SDK base image, plus what treq's boot manifest expects.
FROM docker.io/cloudflare/sandbox:0.4.0
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server git curl ca-certificates \
  && rm -rf /var/lib/apt/lists/*
EXPOSE 8022

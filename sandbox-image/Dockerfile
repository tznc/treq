# Base sandbox image: the setup a treq machine gets at boot, baked in, for
# vendors that start machines from an image. A machine built from it finds
# its manifest version already recorded, so bootstrap is a no-op.
#
# bootstrap.sh and install-sshd.sh are generated; regenerate with
#   deno run --allow-read --allow-write scripts/render-sandbox-image.ts
#
# Build (from the repository root):
#   docker build -f sandbox-image/Dockerfile sandbox-image
#   docker build -f sandbox-image/Dockerfile --build-arg BASE_IMAGE=docker.io/cloudflare/sandbox:0.4.18 sandbox-image
#
# Without bin/treq the build downloads the release, which needs the treq CLI
# digests pinned in boot-manifest.ts: bootstrap refuses a PLACEHOLDER digest.
# .github/workflows/sandbox-image.yml builds and publishes it for release
# tags and nightlies with bin/treq baked in.
#
# The gateway is not started here: its placement id and secret arrive at
# runtime, and the control plane starts it (machine-setup.ts). No CMD or
# ENTRYPOINT is set, so a vendor base image keeps its own.

ARG BASE_IMAGE=ubuntu:24.04
FROM ${BASE_IMAGE}

USER root
# A release or nightly build puts the treq binary it built in bin/; the
# bootstrap rendered for it then skips the download. Empty otherwise.
COPY bin/ /usr/local/bin/
COPY bootstrap.sh install-sshd.sh /opt/treq/
RUN rm -f /usr/local/bin/.gitkeep \
  && sh /opt/treq/bootstrap.sh \
  && sh /opt/treq/install-sshd.sh \
  && rm -rf /var/lib/apt/lists/*

# treq gateway (sshd stays on loopback).
EXPOSE 8022

# Base sandbox image

One image with the setup every treq machine needs: base packages, the treq CLI, jj, git, and the loopback sshd with its `treq` login user. Vendors that start machines from an image (Cloudflare, E2B templates, Fly) can use it in place of bootstrapping at boot.

- `bootstrap.sh` and `install-sshd.sh` are rendered from `boot-manifest.ts` and `ssh-vm-config.ts`. Regenerate them with `deno run --allow-read --allow-write scripts/render-sandbox-image.ts`. A test fails if they are stale.
- `BASE_IMAGE` picks the vendor base (default `ubuntu:24.04`). It must be Debian or Ubuntu.
- A local build needs `bin/treq`: the committed `bootstrap.sh` resolves no treq release, so it fails closed.
- The gateway is started per placement by the control plane, not by the image.

`.github/workflows/sandbox-image.yml` publishes it to `ghcr.io/tznc/treq-sandbox` from `release.yml` (`v<version>`), or by hand (`nightly`): a multi-arch tag on `ubuntu:24.04` and an amd64 `<tag>-cloudflare` tag on Cloudflare's base. Those builds bake in the treq CLI built from the same commit (`bin/treq`), so they need no pinned digests.

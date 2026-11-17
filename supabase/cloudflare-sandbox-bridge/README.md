# treq sandbox bridge (Cloudflare)

Cloudflare Sandboxes run inside a Worker and have no account-level REST API, so treq talks to this small Worker in the owner's account.

Treq deploys it for you: saving a Cloudflare key (`<account id>:<api token>`) in Settings → Provider keys runs `provisionBridge` in `supabase/functions/_shared/remote/cloudflare-adapter.ts`. It creates the `treq-sandbox-types` KV namespace, uploads this Worker with a fresh `BRIDGE_TOKEN`, enables workers.dev, and links one container app per size to Cloudflare's base image `docker.io/cloudflare/sandbox`. Saving again redeploys.

After changing `src/index.ts` or bumping `@cloudflare/sandbox`, run `scripts/build-cloudflare-bridge.sh` and commit the regenerated `cloudflare-bridge.generated.ts`. Keep `BRIDGE_IMAGE` on the same version as the SDK.

To deploy by hand instead: `npm install`, create a KV namespace and put its id in `wrangler.jsonc`, `wrangler secret put BRIDGE_TOKEN`, `npm run deploy`, then save `https://treq-sandbox.<you>.workers.dev#<token>` as the key.

Sizes: Small runs on `standard-3` (2 vCPU / 8 GiB / 16 GB), Medium on `standard-4` (4 vCPU / 12 GiB / 20 GB). The base image has neither git nor sshd: bootstrap installs its base packages and machine setup installs sshd on each cold start, or build `sandbox-image/Dockerfile` on this base to bake them in. A container that sleeps restarts on a fresh disk.

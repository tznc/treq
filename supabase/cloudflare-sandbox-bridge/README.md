# treq sandbox bridge (Cloudflare)

Cloudflare Sandboxes run inside a Worker and have no account-level REST API, so treq talks to this small Worker in your account.

1. `npm install`
2. `wrangler kv namespace create TYPES` and put its id in `wrangler.jsonc`.
3. `wrangler secret put BRIDGE_TOKEN` (a long random string).
4. `npm run deploy`
5. In treq, Settings → Provider keys → Cloudflare, paste `https://treq-sandbox.<you>.workers.dev#<token>`.

Sizes: Small runs on `standard-3` (2 vCPU / 8 GiB / 16 GB), Medium on `standard-4` (4 vCPU / 12 GiB / 20 GB). There is no Large or Extra large.

A container that sleeps restarts on a fresh disk. Push your work before a workspace goes idle.

# Treq devcontainer

Install Docker (Docker Desktop or an equivalent engine) and the VS Code
Dev Containers extension, then open the repository and run **Dev Containers:
Reopen in Container**. This configuration also works with GitHub Codespaces.

The Debian Bookworm container includes Node 24, npm, Rust stable, Git, GitHub CLI,
Jujutsu 0.40.0, and the Tauri v2 Linux build dependencies. Creation runs `npm ci`
and installs the Jujutsu CLI; its first Rust compilation can take several minutes.
The native test addon is built when you run integration tests.

Run commands from the repository root:

```bash
npm run dev -- --host 0.0.0.0       # frontend preview, forwarded port 1420
npm run storybook -- --host 0.0.0.0 # component preview, forwarded port 6006
npm test                          # unit + integration tests, builds NAPI
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

The browser preview does not provide the native Tauri command bridge. The
container supports Linux compilation and NAPI integration tests. Running the
desktop app (`npm run tauri dev`) requires a Linux graphical display, which this
configuration does not provide. Use the host environment for desktop GUI work,
macOS/Windows builds, signing, and mobile SDKs.

`node_modules` and the workspace-root Cargo `target` directory use container
volumes to keep Linux binaries separate from host builds. The NAPI loader at
`src-tauri/target/index.js` is generated in the checkout; rebuild the addon when
switching between host and container integration tests. Re-run
`bash .devcontainer/setup.sh` after dependency changes. Rebuild the container to
pick up image, feature, or system-package changes.

`devcontainer-lock.json` pins the feature implementations. To update those pins,
run `devcontainer features upgrade --workspace-folder .` with the Dev Containers CLI,
then rebuild.

Open a standalone checkout or clone into a container volume. A Treq/Jujutsu
workspace whose `.git` or `.jj` metadata points outside the mounted repository
cannot access that external metadata inside the container.

Supabase services and service QA are separate setups; this container does not
start them or expose the host Docker socket. See
[`supabase/docker/README.md`](../supabase/docker/README.md) for that environment.

System packages follow the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/),
and configuration follows the [Dev Container specification](https://containers.dev/implementors/json_reference/).

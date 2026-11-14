# Mobile QA

Screenshots of the mobile app's GitHub screens, taken the way
`qa/18-ssh-remote.spec.ts` drives the desktop app: the real app with
gpui-playwright, a fresh home whose `~/.ssh` holds the host's key, and a real
SSH host whose `treq` answers every read. A fake `gh` on the host stands in
for GitHub.

```bash
# The app (its desktop build) and the host's headless CLI.
cargo build -p treq --example mobile --features mobile-e2e
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin treq --target-dir target/cli

# The host, with the fake gh. Export what it prints.
QA_SSH_FAKE_GH=1 e2e/desktop/qa/ssh-server/up.sh

cd e2e/desktop
xvfb-run --auto-servernum --server-args="-screen 0 1280x900x24" npm run qa:mobile
```

Captures go to `QA_OUT` (default `qa/.shots/`), each with a `.json` of what
to check, as in `qa/`. The page is a 420×860 phone: one pane, not the
tablet split. Behind a TLS-intercepting proxy on loopback, also pass
`QA_SSH_PROXY` and `QA_SSH_CA` to `up.sh` (the image is then built through it).

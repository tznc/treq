# Treq GPUI (experimental mobile app)

The Treq mobile app rebuilt with a native [GPUI](https://www.gpui.rs) interface instead of the React shell. It renders through [tauri-plugin-gpui](https://github.com/ziinc/tauri-gpui) inside an ordinary Tauri Android app. Components come from [gpui-kit](https://crates.io/crates/gpui-kit).

It is a separate app (`dev.treq.gpui`, "Treq GPUI"), so it installs next to the regular Treq mobile build. Repository work uses treq's own Rust code: `treq_lib` is linked directly (with its `embed` feature). Screens call the SSH transport, the remote command protocol and the settings database without going through IPC. The Supabase sign-in and the managed-instance control plane, which exist only in TypeScript in the React app, are ported to Rust in `src/auth.rs`, `src/control_plane.rs` and `src/managed.rs`.

[`SPEC.md`](SPEC.md) describes the React mobile shell this app replaces.

## Install the debug build

Every push that touches `mobile-gpui/` or `src-tauri/` runs the **Mobile GPUI** workflow. Download its `treq-gpui-debug-apk` artifact, unzip it and install the APK on a device with USB debugging enabled:

```bash
adb install -r treq-gpui-debug.apk
```

Or copy the APK to the phone and open it, allowing installs from that source. The APK is signed with the debug key and contains arm64 (phones) and x86_64 (emulators) libraries. The workflow also uploads emulator screenshots (`treq-gpui-emulator-screenshots`).

## What works

| Area | Status |
|---|---|
| Sign in | Browser sign-in returning through `treq://auth/callback`. The session is restored on launch, and refreshed tokens are saved. |
| Managed instance | Wake, wait, device-key registration, certificate issue, renewal at 80% of the lifetime, relay token sync, and cutoff with reauthentication. |
| SSH hosts | Add a host with explicit host-key trust. Shows this device's public key for `authorized_keys`. |
| Review | Repository picker, workspaces, workspace status and changed files, diff hunks, working-copy content, commits, conflicts. |
| Mutations | Create workspace, rebase, commit, push, resolve conflicts, start/stop the agent and send it input. Each runs once per idempotency key and asks for a second tap before running. An ambiguous result is reported, never retried blindly. |
| Agent | Status and log polling, start, input, stop. |
| Restore | Reopens the last host, repository and workspace on launch; refetches after returning to the foreground. |
| Terminal | Persistent sessions on the host (list, start, reattach, detach, stop), a VT100/xterm grid with colors and scrollback, the soft keyboard, and a key toolbar (Esc, Tab, Ctrl+C/D/Z, arrows). |

## Develop

```bash
# once: treq_lib embeds the desktop frontend, which this app does not use
mkdir -p ../src/dist && echo '<!doctype html>' > ../src/dist/index.html

cargo run                      # desktop window, for quick UI iteration
cargo test --lib

npm install
npm run tauri -- android init  # generates gen/android (not committed)
npm run tauri -- android build --debug --apk --target aarch64
```

The UI font is Inter (OFL 1.1, `assets/fonts/OFL.txt`), bundled so bold and semibold weights render on Android, where the system font is a single variable face.

Builds use the production Supabase project. Set `TREQ_GPUI_ENV=dev` at build time to use a local Supabase stack. The app is a separate Cargo workspace: tauri-plugin-gpui needs tauri 2.12, and this keeps the desktop app's lockfile unchanged.

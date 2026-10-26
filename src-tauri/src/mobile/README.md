# Treq mobile (GPUI)

The Treq mobile app rebuilt with a native [GPUI](https://www.gpui.rs) interface instead of the React shell. It renders through [tauri-plugin-gpui](https://github.com/ziinc/tauri-gpui) inside an ordinary Tauri Android app. Components come from [gpui-kit](https://crates.io/crates/gpui-kit).

It is the Treq mobile app (`com.treq`, the same identifier as the desktop app), built from `src-tauri` with the `mobile-ui` feature (`tauri.ios.conf.json` and `tauri.android.conf.json` hold the mobile-only config). It is this module of `treq_lib`, so screens call the SSH transport, the remote command protocol and the settings database without going through IPC. The Supabase sign-in and the managed-instance control plane, which exist only in TypeScript in the React app, are ported to Rust (`managed.rs` here).

[`SPEC.md`](SPEC.md) describes the React mobile shell this app replaces.

## Install the debug build

The **Mobile GPUI** workflow builds it on manual dispatch. Download its `treq-gpui-debug-apk` artifact, unzip it and install the APK on a device with USB debugging enabled:

```bash
adb install -r treq-gpui-debug.apk
```

Or copy the APK to the phone and open it, allowing installs from that source. The APK is signed with the debug key and contains arm64 (phones) and x86_64 (emulators) libraries. The workflow also uploads emulator screenshots (`treq-gpui-emulator-screenshots`).

The [nightly release](https://github.com/tznc/treq/releases/tag/nightly) also carries `treq-nightly-android-arm64.apk` (arm64 only), and nightly iOS builds go to TestFlight when the signing secrets are set. It also carries `treq-nightly-ios-simulator-arm64.zip`, an iOS simulator build (Apple silicon Macs; it does not install on a device): unzip it and run `xcrun simctl install booted <app>`.

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

From the repository root:

```bash
cargo run -p treq --example mobile --features mobile-ui  # desktop window, for quick UI iteration
cargo test -p treq --lib --features mobile-ui mobile::

npm ci
npm run tauri -- android init  # generates src-tauri/gen/android (not committed)
scripts/mobile/apply-icons.sh  # Treq icon (init uses the Tauri default)
npm run tauri -- android build --features mobile-ui --debug --apk --target aarch64
```

The UI font is Inter (OFL 1.1, `src-tauri/assets/fonts/Inter-OFL.txt`), bundled so bold and semibold weights render on Android, where the system font is a single variable face.

Builds use the production Supabase project. Set `TREQ_GPUI_ENV=dev` at build time to use a local Supabase stack.

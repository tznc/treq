<!-- Survey of the React mobile shell this app replaces (treq @ 4672bf6). Kept as the port's reference; file:line references point at that revision. -->

# Treq mobile UI: spec for the GPUI port (read-only survey of /home/user/treq @ 4672bf6)

The phone flow is: sign in, pick an endpoint, pick a repository, then work in a workspace (diff, commits, conflicts, agent, terminal). All repository data reaches the phone through four Tauri commands:
- `remote_dispatch_over_ssh`
- `remote_dispatch_mutation_over_ssh`
- `remote_pty_list_persistent_sessions`
- `remote_pty_reattach` (plus the small `remote_pty_*` I/O commands)

Everything about Supabase (auth, the control plane, certificate renewal) lives only in TypeScript and has to be ported.

## 0. Problems in the current code that the port should not copy

1. **Workspace name is sent where the VM expects a numeric id.** `RemoteRepoScreen.tsx:90` passes `ws.workspace_name`. On the VM, `core/remote.rs:1811-1821 workspace_id()` rejects anything that isn't a positive integer (`invalid_arguments: workspace must be a positive numeric id`).
   - Affected requests: `InspectWorkspace`, `ListChanges`, `DiffFile`, `ReadFile`, `ListCommits`, `ListConflicts`, `WorkspaceChangeMarker`, `RebaseWorkspace`, `CreateCommit`, `GitPush` and `AgentStart` (which resolves the path from the id, `remote.rs:2335`).
   - Desktop sends `String(ws.id)` (`src/lib/repository-adapter.ts:150,258,...`).
   - These take the workspace as a free string instead: `AgentStatus/Input/Stop/Logs` and `Pty*` (`remote.rs:2347-2424`).
2. **`ListCommits` returns an object, not an array.** It returns `jj::JjLogResult {commits, target_branch, workspace_branch, ...}` (`jj.rs:1038`). `CommitsScreen` (`RemoteRepoScreen.tsx:337-352`) types it as `JjLogCommit[]` and calls `.map`. Read `.commits`.
3. **Terminal working directory is probably wrong.** `local_db::Workspace.workspace_path` is relative to `<repo>/.treq/workspaces/` (`remote.rs:1792-1808`). `RemoteTerminalScreen.tsx:50` uses it as an absolute working directory. Worth checking on a real VM.
4. **Feature-preview gate.** Every remote command handler calls `feature_preview::require(.., PreviewFeature::RemoteSsh)`.
   - Release builds default to `package.json featureFlags.remoteSsh = false` (`core/feature_preview.rs:56-74`); debug builds default to true.
   - Mobile has no UI to set the `feature_preview.remoteSsh` setting, so release mobile builds fail with `Feature preview 'remoteSsh' is disabled`.
   - Linking core directly skips this gate unless you call `core::feature_preview::require(&db, ..)` yourself.
5. **Refreshed Supabase tokens are never saved.** The `supabase_session` setting is written only after `exchangeToken`. Tokens that supabase-js refreshes later are not written back (`authStore.ts:39-51,124`), so a restore can fail once refresh tokens rotate.
6. **No way to add an SSH host on the phone.** Mobile only lists records already stored under `remote_user_managed_endpoints`. The add form (`RemoteUserManagedSetupPanel` / `RemoteSetupDialog`) is mounted only by desktop `Dashboard.tsx:2726`.
7. **No ambiguous-mutation dialog on mobile.** `RemoteAmbiguousMutationDialog` is mounted only in `Dashboard.tsx:2793,3359`. Mobile shows an ambiguous result as inline red text.
8. **Some actions run on a single tap.** Start agent, Send input and terminal Stop have no arm/confirm step. Rebase, Commit, Push bookmark, Create workspace, Resolve and Stop agent do.
9. **A failed session restore blocks the remembered managed session.** The restore waits for `user` to be non-null (`RemoteConnectPanel.tsx:113`). If `restoreSession` fails, it never runs.
10. **Status response doesn't match the Rust type.** `remote-instance` `status` returns `endpoint` as `{id, hostname, port, username, source}`, with `source` a raw DB column (`supabase/functions/remote-instance/index.ts:248-261`). That will not deserialize into Rust `core::remote_control_plane::InstanceStatusResponse.endpoint: Option<SshEndpoint>` (`remote_control_plane.rs:189-192`). Use a lenient struct. Only `issue_certificate` returns a full `SshEndpoint`.

---

## 1. Screens and flows, in navigation order

### 1.0 Shell selection and layout
- `App.tsx:16` picks the shell once at startup: `shouldUseMobileShell(window.location.search)`.
  - `mobile-platform.ts:13-27`: true when `import.meta.env.TAURI_ENV_PLATFORM` is `android` or `ios`, or in a dev build with `?shell=mobile`.
- `MobileShell` (`MobileShell.tsx:24-46`) is one scrolling column:
  - A sticky header with the title "Treq" and the account control.
  - `RemoteConnectPanel` below it.
  - `LocalRepositoryPreview`, only when this is not a mobile build (desktop dev preview).
- On mount it starts the cutoff listener (`useRemoteCutoffStore.startListening()`, lines 29-32); see §4.
- Global effects run from `AppStoreEffects`. The one that matters here is `restoreSession()` (`AppStoreEffects.tsx:60-62`).
- Global SWR config: `revalidateOnFocus: false`, `errorRetryCount: 1` (`lib/swr-cache.ts:84-87`). SWR's default revalidate-on-reconnect still applies.

### 1.1 Sign-in (header, `MobileAccountControl`, `MobileShell.tsx:54-122`)
- **Signed out:** a "Sign in" button, disabled while `authStore.loading`.
  - Tapping it calls `openUrl(`${WEB_URL}/sign-in?source=desktop`)`, which opens the system browser (`authStore.ts:55-57`).
  - Errors show as small red text under the button.
- **Signed in:** the user's email and a "Sign out" button.
- **Deep-link return path:**
  1. The web page `web/src/pages/auth/callback.tsx:81-91` calls the RPC `create_desktop_token` and navigates to `treq://auth/callback?token=<t>`.
  2. `listenForAuthCallbacks` (`auth-deep-link.ts:38-50`) registers `onOpenUrl` and also checks `getCurrent()` for the launch URL.
  3. It accepts only scheme `treq:` with `host+pathname === "auth/callback"` and reads `token` (lines 15-31).
  4. Tokens already handled are de-duplicated in a module-level Set (line 35).
  5. It calls `exchangeToken(token)`; errors show under the button.
- **Sign out** (`authStore.ts:58-69`), in order:
  1. Best-effort `remote_cut_off_managed`.
  2. `supabase.auth.signOut()`.
  3. Clear user/session/subscription.
  4. `set_setting("supabase_session", "")`.
- There are no confirmation prompts anywhere in the sign-in flow.

### 1.2 Remote panel, endpoint step (`RemoteConnectPanel.tsx:192-289`, `MobileEndpointPicker` in `MobileRemotePickers.tsx:16-73`)
Shown when there is no endpoint and the status is not `secure_storage_unavailable`.
- **"Connect to managed instance"**: disabled when signed out or while connecting. Signed out, it shows the hint "Sign in to connect to your Treq-managed instance."
- **"SSH hosts on this device"**: the records from the `remote_user_managed_endpoints` setting. Each row shows `display_name` and `username@hostname:port`; disabled while connecting. Tapping a row calls `connectUserManaged` (synchronous, no network, sets status `connected`).
- **Progress line:** `"{step}..."`. The step text is one of:
  - "Checking the managed instance"
  - "Waking the managed instance"
  - "Waiting for the managed instance"
  - "Getting a certificate"
  - "Reauthenticating"
- **Error:** red text with `state.error`. The "no instance" error reads "No managed instance for this account yet. Set one up from Treq on desktop, then connect here." (`useMobileRemoteConnection.ts:64`).
- **Secure-storage alert** (`RemoteConnectPanel.tsx:320-351`): shown when the device-key error starts with `secure_storage_unavailable:` (the prefix is stripped).
  - Title "Secure storage isn't set up on this device", plus an explanation about Face ID / Touch ID / fingerprint enrolment.
  - A "Try again" button that calls `connectManaged()` again.

#### Managed connect state machine (`useMobileRemoteConnection.ts:207-245`, using `lib/managed-ssh-connection.ts`)
1. Call control-plane `status` (§3.3) and record `instanceState`.
2. `instance == null` → error `NO_MANAGED_INSTANCE_MESSAGE`. Mobile never provisions: `ensureInstance` always rejects (line 149).
3. `status == "suspended"` → `wakeManagedInstance`:
   - `wake` with `idempotency_key = wake-${instanceId}-${uuid}` (`managed-ssh-connection.ts:112-117,333-350`).
   - Then poll readiness, then step 5.
4. `status == "waking"` → `waitForInstanceReady`: poll `status` every 2 s with a 10 min timeout (lines 154-189).
   - Throws on `failed`/`deleted`: "Managed instance provisioning failed (status: X)."
   - Throws on timeout: "Timed out waiting ... (last status: X)."
5. `connectExistingReadyInstance` (lines 303-320). If the instance is not `ready`, throws "Managed instance is not ready (status: X)."
   - **Read public key:** `ensure_mobile_device_key`, which returns `{public_key, fingerprint_sha256}`.
   - **Register:** `register_client_key` with `{public_key, comment: "treq-mobile-device", idempotency_key: "register:<fingerprint>"}` (`useMobileRemoteConnection.ts:47,143-148`).
   - **Issue:** `issue_certificate` with `{instance_id, key_id}`.
   - **Compose the endpoint** (`managed-ssh-connection.ts:199-212`): take the server endpoint and replace `authentication` with `{type:"certificate", key_reference:"keystore:device", certificate}`.
   - **Relay token:** if `endpoint.transport.type == "relay"`, run `prepareRelay`, i.e. `ensureRelayAccessTokenSync()` (§3.4), before activating.
   - **Activate:** store the endpoint. `stopRenewal()` runs first.
   - **Start renewal:** lease `{instanceId, keyId, endpointId, serial, certificate, issuedAt: now, expiresAt: Date.parse(expires_at)}`; see §3.5.
6. `adopt`: status `connected`, `choice = {kind:"managed"}`.

#### User-managed endpoint (`remote-endpoints.ts:144-157`)
The record is turned into this `SshEndpoint`:
```
{id: record.id, instance_id: null, source: {type:"user_managed"}, hostname, port, username,
 host_keys: [{algorithm:"unknown", fingerprint_sha256: record.host_key_fingerprint, comment:null}],
 authentication: {type:"public_key", key_reference: record.auth_identity_reference}}
```
Host-key trust is enforced in Rust by exact string match on `SHA256:...` (`remote_ssh_transport.rs:379-437`). A mismatch rejects the connection and is never bypassed. It reaches the UI as a connection or exec error string from the first dispatch (e.g. ProbeRepo): "ssh connection failed: ..." or "host key mismatch for endpoint X".

**Host-trust confirmation (desktop form only, `RemoteUserManagedSetupPanel.tsx`):**
- Fields: display name, hostname, port (default 22), username, expected fingerprint `SHA256:...`, auth identity reference, optional alias.
- "Autofill from alias" calls `resolve_ssh_config_alias`.
- "Continue" shows the "Confirm host trust" box: "You are about to trust user@host:port with host key fingerprint X. Treq will reject this endpoint if the presented key ever changes, unless you confirm a new fingerprint yourself."
- Buttons: "Trust and connect" / "Cancel".
- Desktop then saves the record with `id: "user-managed-${Date.now()}"` and `created_at` set to the current ISO time (`Dashboard.tsx:1082-1097`).

### 1.3 Connected header, status banner, cutoff
- "Connected to {hostname}:{port}" and a **"Switch host"** button (`RemoteConnectPanel.tsx:174-179`): disconnect, stop renewal, close the repo, clear the error, and `clearMobileSession()`.
- **`RemoteStatusBanner`** (`remote/RemoteStatusBanner.tsx`), state computed at `RemoteConnectPanel.tsx:181-190`:
  - Precedence: cutoff, then `connecting`, then (managed only) the mapping from `instanceState`, otherwise `online`.
  - Mapping: `waking`/`suspended` → "Waking managed VM..."; `degraded`/`failed` → "Degraded - some checks are failing"; `reprovisioning` → "Reconnecting..."; not connected → "Offline"; otherwise `online`, which shows no banner.
  - Actions: "Wake now" on waking, "Reconnect" on offline/reconnecting (both call `connectManaged`). "Refresh" calls `refresh()`; it is hidden while cut off.
  - Cutoff label: "Credential cutoff - reauthenticate to continue", detail "Access ended (session ended)." (reason with `_` replaced by spaces).
- **Cutoff block** (lines 291-318) replaces all repository UI while `cutoffs[endpoint.id]` is set.
  - Title "Remote access is blocked" and text about the certificate being revoked or expired.
  - "Reauthenticate" button, disabled while a step is running. It calls `reauthenticateManagedInstance` (`managed-ssh-connection.ts:364-376`): register key → issue certificate → activate → `clearCutoff`.
  - `clearCutoff` = `remote_clear_cutoff` plus removing the local entry. It runs only after issuance succeeds; on failure the error text shows and the block stays.
  - The instance id comes from `lease.instanceId ?? endpoint.instance_id`. With neither (a user-managed endpoint) the button does nothing.
- **`refresh()`** (lines 164-172): if managed and not connected, connect again and then bump `epoch`; otherwise just bump `epoch`, which remounts `RemoteRepoScreen` so every screen refetches.

### 1.4 Repository picker (`MobileRepoPicker`, `MobileRemotePickers.tsx:81-144`)
- **"Repositories on this host"**: saved records filtered by `endpoint_id` plus generation (managed: `source.generation`; otherwise 0).
  - Each row shows `display_name` and `canonical_remote_path`; tapping opens it.
- A free-text input "Repository path on the instance" and an **"Inspect repository"** button. The button shows "Inspecting..." while busy and is disabled when the input is empty.
- **`openRepository`** (`RemoteConnectPanel.tsx:70-100`):
  1. `ProbeRepo {repo: path}` → `RemoteRepoProbe {host, path, exists, is_repo, needs_clone}`.
  2. If `!exists || !is_repo`, show the error "No repository found at {path} on this host."
  3. Otherwise open the repo screen and, fire-and-forget, `upsertSavedRemoteRepository({endpoint_id, endpoint_generation, remote_path})` (§3.6).
- Once open, a link "{path} · Change repository" closes the repo.

### 1.5 Repository screens (`RemoteRepoScreen.tsx`)
State type (lines 33-40):
```
workspaces | {workspace, name} | {diff, workspace, path} | {commits|conflicts|agent|terminal, workspace}
```
- "← Back" (lines 73-85): from diff/commits/conflicts/agent/terminal it goes to the workspace screen; from the workspace screen it goes to the list.
- Every navigation calls `onScreenChange`, which is persisted (§1.10).

**A. Workspace list** (lines 155-258)
- `ListWorkspaces {repo}` → `local_db::Workspace[]`, plus a "Refresh" button and error text.
- **New-workspace form:** "Branch name" and "Source branch (optional)", then the arm/confirm button "Create workspace" → "Confirm create" → "Working...".
  - Sends `CreateWorkspace {repo, branch_name, source_branch|null, idempotency_key}` through the mutation path.
  - On success: clear the inputs and refetch. "ambiguous" shows "Could not confirm the change applied: {reason}". Errors show inline.
- Empty list: "No workspaces found."
- Each row shows `title || workspace_name` and `branch_name`; tapping opens the workspace screen.

**B. Workspace detail** (`RemoteWorkspaceMutationScreens.tsx:26-304`)
- Reads:
  - `InspectWorkspace {repo, workspace}` → `WorkspaceStatus` (`has_changes`, `has_conflicts`, `conflicted_files`, ...), shown as "Has uncommitted changes/Clean" and "Has conflicts/No conflicts".
  - `ListChanges {repo, workspace}` → `JjFileChange[] {path, status, previous_path, changed_line_count, diff_deferred}`.
  - `WorkspaceChangeMarker`, polled every 15 s → `{operation_id, working_copy_change_id}`, shown as "op {first 12 chars}".
- "Refresh" reloads all three.
- **Mutations** box, each with arm/confirm and inline errors:
  - **Rebase:** input "Rebase onto branch" + "Rebase"/"Confirm rebase" → `RebaseWorkspace {repo, workspace, target_branch, idempotency_key}`.
  - **Commit:** input "Commit message" + "Commit"/"Confirm commit" → `CreateCommit {repo, workspace, message, idempotency_key}`, with `base_change_id` filled in by Rust (`remote.rs:1088-1126`). Clears the message on success.
  - **Push:** "Push bookmark"/"Confirm push" → `GitPush {repo, workspace, idempotency_key}`.
- Navigation buttons: Commits, Conflicts, Agent, Terminal.
- "Changed files" list: `{STATUS} path`; tapping opens the diff. Empty: "No changed files."

**C. Diff / hunk view** (`RemoteRepoScreen.tsx:260-323`)
- `DiffFile {repo, workspace, path}` → `JjDiffHunk[] {id, header, lines[], patch, conflict_style, conflict_regions[]}`. Each hunk renders as a monospace block: the header, then the lines.
- `ReadFile {repo, workspace, path, revision:"WorkingCopy"}` → `JjFileLines {lines, start_line, end_line}`. This is lines 1-300 by default (`remote.rs:1877-1884`) and goes into a collapsed "Working-copy content" section.
- Refresh button. The view is read-only.
- Line colouring rule (from the local preview `MobileHunkView.tsx:12-20`): `+` lines (not `+++`) green, `-` lines (not `---`) red.

**D. Commits** (`RemoteRepoScreen.tsx:325-369`)
- `ListCommits {repo, workspace}`. Each commit shows `short_id`, `description || "(no description)"` and `author_name · timestamp`. Read-only (see §0.2).

**E. Conflicts** (`RemoteWorkspaceMutationScreens.tsx:306-413`)
- `ListConflicts {repo, workspace}` → `string[]` of paths. Empty: "No conflicted files."
- When the list is non-empty, a resolve form appears:
  - "Revision", default `@`.
  - A select: `side1` "Keep ours", `side2` "Keep theirs", `both` "Keep both".
  - Arm/confirm "Resolve" → `ResolveConflict {repo, revision, sides:[side], idempotency_key}`. Note: no workspace field. VM returns `ResolveCommitResult {success, message, change_id, remaining_conflicts}`.
  - Refetch on success.

**F. Agent** (`RemoteAgentScreen.tsx`)
- `AgentStatus {repo, workspace}`, polled every 4 s → `AgentStatusResult {workspace, running, agent, pid, started_at, should_refresh}`.
- `AgentLogs`, polled every 4 s only while `running` → `string`.
- **Not running:**
  - A select over `["claude","codex","cursor-agent","copilot"]` (the VM allow-list is the same, `agent_supervisor.rs:198`) and a "Prompt" textarea.
  - "Start agent" (single tap, disabled when the prompt is empty or busy) → `AgentStart {repo, workspace, agent, prompt, idempotency_key}`, then refetch status. Ambiguous: "Could not confirm the agent started: ...".
  - The VM returns `AgentRecord`, or the error `agent_already_running: ...`.
- **Running:**
  - "Running {agent} (pid N)", "Started {started_at}".
  - "Send input" textarea + button (single tap) → `AgentInput {repo, workspace, input, idempotency_key}`. Clears the input on success, then refetches logs. Ambiguous: "Could not confirm the input was sent: ...".
  - "Stop agent" (destructive arm/confirm) → `AgentStop {repo, workspace}` through the mutation path, with no key. Then refetch status and logs.
- A "Logs" section, open by default, with scrollable pre-wrapped text. A "Refresh" button refetches status.

**G. Terminal** (`mobile/RemoteTerminalScreen.tsx`, `RemoteTerminalPanel.tsx`, `mobile/RemoteTerminalTouchToolbar.tsx`)
- **On open:**
  - `ListWorkspaces`, to pick the working directory from `workspace_path` (falls back to the repo root; see §0.3).
  - `remote_pty_list_persistent_sessions(endpoint, repo, workspace)` → `PtySessionInfo[] {session_name, workspace, label, running}`.
- **List UI:**
  - "Checking for running sessions…", or the load error.
  - "Running sessions" rows: label, "running/stopped", and "Reattach" (disabled when not running).
  - "Start new session" with label `shell-${Date.now().toString(36)}` (`remote-terminal-target.ts:74-79`); launch is `{type:"shell"}`.
- **Full-screen panel** (fixed inset):
  - Header: "{hostname} · {label}", plus "(session ended, exit N)" when ended.
  - "Stop" → `PtyStop {repo, workspace, label}` through the plain read path `remote_dispatch_over_ssh`, then close. Errors show in an overlay.
  - "X Detach" closes only the local channel; the session keeps running.
- **Terminal:** xterm settings fontSize 13, JetBrains Mono, background `#1e1e1e`, scrollback 5000, cursor bar + blink, web-links.
- **Attach sequence** (lines 204-246):
  1. Subscribe to the data and exit events for a fresh local `sessionId = remote-pty-${endpoint.id}-${workspace}-${label}-${Date.now()}` (lines 89-90).
  2. `remote_pty_reattach(sessionId, endpoint, repo, workspace, label, cwd, launch, cols||80, rows||24)`.
  3. Mark ready, fit, then `remote_pty_resize`.
- **Input:** `remote_pty_write(sessionId, data)`. On resize (ResizeObserver): fit, then `remote_pty_resize`.
- **Unmount or detach:** `remote_pty_close(sessionId)`.
- **When the channel exits** (lines 169-199): re-list sessions.
  - Session gone → "ended".
  - Session still running → auto-reattach at most once every 30 s (`AUTO_REATTACH_COOLDOWN_MS`, line 44).
  - Otherwise, or if the host is unreachable → "detached" overlay: "Connection to the remote session was lost. The session may still be running on the host." with "Reattach" / "Close". Errors use the same overlay.
- Loading overlay text: "Reattaching…" or "Starting remote session…".
- **Touch toolbar** (lines 8-21), each key writes the literal bytes:
  - Esc `\x1b`, Tab `\t`, Ctrl+C `\x03`, Ctrl+D `\x04`, Ctrl+Z `\x1a`
  - ↑ `\x1b[A`, ↓ `\x1b[B`, → `\x1b[C`, ← `\x1b[D`

### 1.6 Mutations and confirmation rules
- **`MutationButton`** (`remote/RemoteScreenControls.tsx:28-69`): first tap arms it (label becomes `confirmLabel`), second tap runs it ("Working..."). Losing focus disarms it.
- **Result handling** (lines 72-79, `remote-dispatch.ts:340-367`):
  - `{status:"applied", value}` and `{status:"already_applied"}` count as success.
  - `{status:"ambiguous", reason}` shows "Could not confirm the change applied: {reason}".
- **Idempotency keys** (`lib/remote-idempotency.ts:16-57`): one instance per screen.
  - `keyFor(prefix, inputs)` returns `${prefix}:${uuid}`, reused while the fingerprint `JSON.stringify([prefix, ...inputs])` has no confirmed outcome.
  - `settle()` forgets the key unless the result was ambiguous. A thrown error keeps the key.
  - Prefixes and their inputs:
    - `create-workspace` [repo, branch, source]
    - `rebase` [repo, ws, target]
    - `commit` [repo, ws, msg]
    - `push` [repo, ws]
    - `resolve` [repo, ws, rev, side]
    - `agent-start` [repo, ws, agent, prompt]
    - `agent-input` [repo, ws, input]
- **Verify-before-retry in Rust** (`core/remote.rs:970-1084`):
  - A transport failure triggers a typed verification read.
  - Outcome AlreadyApplied → `AlreadyApplied`; NotApplied → resend with the same key; Ambiguous → `Ambiguous`.
  - Structured CLI errors, invalid JSON and cutoff are returned as errors directly.
- **Desktop ambiguous dialog** (not on mobile): title "Remote change could not be verified", description "A network interruption happened while a mutation was in flight. Treq did not retry automatically because the remote state is ambiguous.", the reason, and "Dismiss".

### 1.7 Local repository preview (desktop dev only, not on a phone)
`MobileShell.tsx:124-216`, using:
- `get_setting("lastRepoPath")`
- `get_workspaces`
- Tabs "Changes" (`MobileDiffView`), "History" (`MobileCommitView` → `LinearCommitHistory` → commit drill-down via `get_commit_diff`) and "Conflicts" (`MobileConflictView` → `get_workspace_status`, `get_workspace_diff`, `get_workspace_file_hunks`).
- `get_workspace_file_hunks_batch` for uncommitted files.
- Agent review comments (`list/resolve/delete/apply_agent_review_*`).
- File list: collapsible, chevron, `STATUS path`, then hunks; empty text "No diff available for this file."
- Conflict regions: "Conflict N of M". Line colours: left green, right blue, base amber, markers bold.
- Large diffs: `too_large_to_render` shows `render_block_reason`.

### 1.8 App resume (`hooks/useAppResume.ts`, `RemoteConnectPanel.tsx:150-161`)
- **Triggers:** `visibilitychange` back to visible (with how long the app was hidden) and `window.online`.
- **`refreshAfterResume`** (`useMobileRemoteConnection.ts:294-321`):
  - Returns "unchanged" if not on a managed endpoint or currently connecting.
  - Returns "failed" if the endpoint is cut off; only Reauthenticate restores it.
  - If not connected, runs `connectManaged()`.
  - Otherwise fetches `status` and treats the connection as stale when any of these hold:
    - there is no lease
    - `renewalDelayMs(...) == 0`
    - the instance is not `ready`
    - the status endpoint id differs from the current one
    - the generation differs
  - Stale → `connectManaged()` → "reconnected" (or "failed").
- **Remount:** after the check, bump `epoch` (remount and refetch everything) when the outcome is "reconnected", the trigger was `online`, or the app was hidden for at least `REMOUNT_AFTER_HIDDEN_MS = 30_000` (line 36).

### 1.9 Restore on launch (`RemoteConnectPanel.tsx:102-136`)
- Load the snapshot (§3.7) once.
- Managed: wait until `user` is set, then `connectManaged()`.
- User-managed: look the id up in `remote_user_managed_endpoints`; if missing, do nothing.
- Then `openRepository(endpoint, repoPath, screen)`. The screen only names remote objects; every screen refetches.
- `restoringRef` suppresses saves during the restore.

### 1.10 Persisting the session
After a connect, a repo change or a screen change, the snapshot is saved, but only when `choice` is set, status is `connected` and no restore is running (lines 139-148).

---

## 2. Commands: TS wrapper → Tauri command → Rust implementation

`commands` is a private module (`lib.rs:10`) and `AppState` is `pub(crate)` with private fields (`lib.rs:81-116`). A separate crate therefore cannot call command handlers or build `AppState`. It must call the `pub` items under `treq_lib::core::*` and `treq_lib::db` directly. Below, **Thin** means the handler only delegates to a pub core function.

| TS wrapper (file:line) | Command, args (camelCase over IPC) → return | Rust handler | Core function to call directly | Tauri needs |
|---|---|---|---|---|
| `getSetting` api.ts:142 | `get_setting {key}` → `string \| null` | commands/settings.rs:26 | `db::Database::get_setting(&key)` (db.rs:200) | `State<AppState>` (db mutex). Thin |
| `setSetting` api.ts:150 | `set_setting {key, value}` → void | settings.rs:41 | `Database::set_setting` (db.rs:213) | AppState. Thin |
| `getSettingsBatch` api.ts:146 | `get_settings_batch {keys}` → `Record<string, string\|null>` | settings.rs:32 | `Database::get_settings_batch` (db.rs:221) | AppState. Thin |
| `ensureMobileDeviceKey` api.ts:705 | `ensure_mobile_device_key {}` → `DeviceKeyInfo {public_key, fingerprint_sha256}` | commands/remote.rs:59-66 | `core::remote_device_key::ensure_device_key(&AppHandle)` (remote_device_key.rs:167-171) | `State<AppState>` (preview gate) **and `tauri::AppHandle`** with `tauri-plugin-keystore` + `tauri-plugin-biometric`. **Not portable as-is**, see below |
| `resolveSshConfigAlias` api-remote-ssh.ts:11 (desktop form) | `resolve_ssh_config_alias {alias}` → `ResolvedSshAlias` | remote.rs:25-32 | `core::remote_ssh_config::resolve_alias` | AppState (gate). Thin |
| `remoteDispatchOverSsh` api-extra.ts:804 | `remote_dispatch_over_ssh {endpoint: SshEndpoint, request: TreqCommandRequest}` → JSON | commands/remote_control.rs:55-74 | `core::remote::execute_remote_command::<serde_json::Value>(&pool, &endpoint, request, ExecLimits::default(), &CancellationToken::new())` (remote.rs:2833-2886) | `State<RemoteExecState>`, `State<AppState>` (gate). Thin. Error string is `RemoteCommandError` Display |
| `remoteDispatchMutationOverSsh` api-extra.ts:818 | `remote_dispatch_mutation_over_ssh {endpoint, request}` → `{status:"applied",value}\|{status:"already_applied"}\|{status:"ambiguous",reason}` | remote_control.rs:244-271 (DTO 222-236) | `core::remote::retry_after_reconnect::<Value,_>(&pool, &ep, req, limits, &cancel, \|v\| pool.metrics.record_post_reconnect_verification(v))` → `MutationRetryOutcome` (remote.rs:930-944) | RemoteExecState, AppState (gate). Thin. Re-implement the 3-variant DTO yourself |
| `remoteForceCutoff` api-extra.ts:840 | `remote_force_cutoff {endpointId, reason: "session_ended"\|"key_revoked"\|"instance_inaccessible"\|"certificate_expired"}` | remote_control.rs:132-155 | `pool.force_cutoff(id, CutoffReason)` (transport.rs:821), then `RemotePtyManager::close_all_for_endpoint(id)` (remote_pty.rs:367), then **emit `remote://cutoff`** | `AppHandle` (emit) + RemoteExecState + RemotePtyState. Thin, but do the emit yourself |
| `remoteCutOffManaged` api-extra.ts:850 | `remote_cut_off_managed {}` | remote_control.rs:161-179 | `pool.cut_off_managed(SessionEnded)` → `Vec<endpoint_id>`; for each, `close_all_for_endpoint` and emit | AppHandle + both states |
| `remoteClearCutoff` api-extra.ts:858 | `remote_clear_cutoff {endpointId}` | remote_control.rs:185-192 | `pool.clear_cutoff(id)` (transport.rs:881) | RemoteExecState. Thin |
| `remoteSetRelayAccessToken` api-extra.ts:866 | `remote_set_relay_access_token {token: string\|null}` | remote_control.rs:198-205 | `pool.set_relay_access_token(Option<String>)` (transport.rs:787). A non-empty token also clears the sign-out managed cutoff | RemoteExecState. Thin |
| `remoteCutoffReason` api-extra.ts:876 (unused) | `remote_cutoff_reason {endpointId}` → reason \| null | remote_control.rs:210-216 | `pool.cutoff_reason(id)` | RemoteExecState |
| `remoteTransportMetrics` (unused) | `remote_transport_metrics` → `SshTransportMetricsSnapshot` | remote_control.rs:279-284 | `pool.metrics_snapshot()` | RemoteExecState |
| `remotePtyListPersistentSessions` api-extra.ts:216 | `remote_pty_list_persistent_sessions {endpoint, repo, workspaceId: string\|null}` → `PtySessionInfo[]` | commands/remote_pty_commands.rs:256-278 | `execute_remote_command::<Value>(.., TreqCommandRequest::PtyList{repo, workspace})` | RemoteExecState, AppState (gate). Thin |
| `remotePtyReattach` api-extra.ts:227 | `remote_pty_reattach {sessionId, windowLabel, endpoint, repositoryId, workspaceId, label, remoteWorkingDirectory, launch: PtyLaunchSpec, cols, rows}` → void | remote_pty_commands.rs:287-348 | (1) `execute_remote_command::<String>(.., PtyAttachCommand{repo, workspace, label, remote_dir, launch, cols, rows})` → command line; (2) `RemotePtyManager::create_with_command(RemotePtyBinding{endpoint_id, repository_id, workspace_id, remote_working_directory, local_session_id, window_label}, &ep, &cmd, cols, rows, on_output: Fn(Vec<u8>), on_exit: FnOnce(Option<u32>))` (remote_pty.rs:244-321) | **AppHandle** (event emit inside `tauri_event_handlers`, lines 155-188), RemotePtyState, RemoteExecState, AppState. Replace the emits with your own callbacks plus a UTF-8 stream decoder (`pty::Utf8StreamDecoder` is `pub(crate)`, pty.rs:18; copy its logic, see lines 117-149) |
| `remotePtyCreate` api-extra.ts:138 (desktop path, unused on mobile) | `remote_pty_create {sessionId, windowLabel, endpoint, repositoryId, workspaceId, remoteWorkingDirectory, launch, cols, rows}` | remote_pty_commands.rs:65-104 | `RemotePtyManager::create(binding, &ep, spec, cols, rows, on_output, on_exit)` (remote_pty.rs:219-233) | AppHandle, RemotePtyState, AppState |
| `remotePtyWrite` api-extra.ts:160 | `remote_pty_write {sessionId, data: string}` | rpc.rs:190-201 | `manager.write(id, data.as_bytes())` (remote_pty.rs:325) | RemotePtyState. Thin |
| `remotePtyResize` api-extra.ts:165 | `remote_pty_resize {sessionId, cols, rows}` | rpc.rs:203-215 | `manager.resize(id, cols, rows)` (remote_pty.rs:332) | Thin |
| `remotePtyClose` api-extra.ts:171 | `remote_pty_close {sessionId}` | rpc.rs:217-227 | `manager.close(id)` (remote_pty.rs:344) | Thin |
| `remotePtySessionExists` (unused) | `remote_pty_session_exists {sessionId}` → bool | rpc.rs:229-235 | `manager.session_exists(id)` | Thin |

(Table abbreviation: rpc.rs = commands/remote_pty_commands.rs.)

**Device key (needs porting).**
- `ensure_device_key` and `device_key_provider` need a `tauri::AppHandle` with the keystore and biometric plugins. The helpers `generate_private_key` and `device_key_info` are private (`remote_device_key.rs:43-62`).
- To port:
  - Generate an ed25519 key from a 32-byte `getrandom` seed with `Ed25519Keypair::from_seed`, comment `"treq-mobile-device"`.
  - Store it as OpenSSH private-key text in Android Keystore / iOS Keychain under key `"com.treq.mobile-device-key/device-key"` (line 70), and require biometrics first (lines 100-122).
  - Public half: `to_openssh()`. Fingerprint: `fingerprint(HashAlg::Sha256).to_string()`, i.e. `SHA256:...`.
  - Prefix storage-unavailable errors with `"secure_storage_unavailable:"` (line 40).
- Then build the pool with `SshConnectionPool::new().with_device_key_provider(Arc::new(|| Box::pin(async { load_key() })))`. The type is `DeviceKeyProvider = Arc<dyn Fn() -> Pin<Box<dyn Future<Output=Result<PrivateKey,String>>+Send>>+Send+Sync>` (transport.rs:498-502).
- Endpoints that use key reference `"keystore:device"` (`DEVICE_KEYSTORE_KEY_REFERENCE`, transport.rs:493) resolve through that provider once per new connection. A certificate endpoint must carry the certificate inline (transport.rs:586-592).

**State that must exist.** `lib.rs:481-496` runs `app.manage(AppState)`, `app.manage(RemotePtyState::new(&exec))` and `app.manage(RemoteExecState)`. The equivalents in a standalone app:
- One `Arc<SshConnectionPool>`, with the device-key provider on mobile.
- One `RemotePtyManager::new(pool.clone())` sharing that pool.
- A `db::Database` (`Database::new(app_data/treq.db)` + `init()`, db.rs:24-29) for the settings keys.
- **A tokio runtime.** `RemotePtyManager::create_with_command` calls `tokio::spawn` (remote_pty.rs:291), and russh needs tokio. Run all core async calls inside a tokio runtime; GPUI's executor is not tokio.

**`TreqCommandRequest`** (`core/remote.rs:105-397`):
- serde tag `"kind"`; snake_case fields exactly as in `src/lib/remote-dispatch.ts:85-299`.
- `FileRevision` is `"WorkingCopy" | "Parent"`.
- `PtyLaunchSpec` is tagged `type`: `{"type":"shell"}` or `{"type":"agent","agent":"claude"|"codex"|"cursor_agent","args":[]}` (remote_pty.rs:37-59).
- Mutation and key classification: `is_mutation()` (lines 422-472) and `requires_idempotency_key()` (lines 478-526).
- On the wire it becomes `treq <args>` over an SSH exec channel (transport.rs:1137-1264), limited to 30 s and 8 MiB (lines 1123-1130).
- The VM runs `cli::parse_remote_command_request` → `execute_local_request` (cli/mod.rs:331-615, remote.rs:1824-2426).
- Error strings:
  - `transport_error: ...`
  - `{code}: {message}` (from the CLI's `{"error":{code,message}}`)
  - `invalid_remote_json: ...`
  - `credential_cut_off: endpoint X (reason)` (remote.rs:2809-2823)

**Mobile request → VM result type:**
- `ProbeRepo` → `RemoteRepoProbe`
- `ListWorkspaces` → `Vec<local_db::Workspace>` (local_db.rs:14-35)
- `InspectWorkspace` → `core::workspaces::WorkspaceStatus` (workspaces.rs:137; the flattened `partial` gives `current`, `has_conflicts`, `has_changes`, `commits_ahead`)
- `ListChanges` → `Vec<JjFileChange>`
- `DiffFile` → `Vec<JjDiffHunk>`
- `ReadFile` → `JjFileLines`
- `ListCommits` → `JjLogResult`
- `ListConflicts` → `Vec<String>`
- `WorkspaceChangeMarker` → `{operation_id, working_copy_change_id}`
- `CreateWorkspace` → `Workspace`
- `RebaseWorkspace` → `Workspace`
- `CreateCommit` / `GitPush` → `String`
- `ResolveConflict` → `ResolveCommitResult`
- `AgentStart` → `AgentRecord`
- `AgentInput` → `String`
- `AgentStatus` / `AgentStop` → `AgentStatusResult` (agent_supervisor.rs:39-50)
- `AgentLogs` → `String`
- `PtyList` → `Vec<PtySessionInfo>` (pty_remote_supervisor.rs:98-106)
- `PtyAttachCommand` → `String`
- `PtyStop` → `()`

---

## 3. Logic that exists only in TypeScript (port all of it)

### 3.1 Config and environment
- **Where the values come from:** `src/lib/supabase.ts:1-17` reads `package.json` → `env[PROD ? "prod" : "dev"]` (package.json:23-38). There are no `.env` files and no `import.meta.env` Supabase variables.
  - prod: `webUrl https://treq.dev`, `supabase.url https://xnlljmfiqyumiyexydyl.supabase.co`, `anonKey sb_publishable_pLkrXd6cs1V7Ot6Dnowmtw_KBgFf88E`
  - dev: `webUrl http://localhost:3001`, `url http://127.0.0.1:54321`, `anonKey sb_publishable_ACJWlzQHlZjBrEguHvfOxg_3BJgxAaH`
- Rust already embeds package.json via `include_str!` in `core/feature_preview.rs:6`; you can do the same.
- Other build-time values: `TAURI_ENV_PLATFORM` (vite `envPrefix`, vite.config.ts) and `import.meta.env.DEV/PROD`.
- The supabase client is built with `autoRefreshToken:false, persistSession:false`.

### 3.2 Supabase auth (`stores/authStore.ts`)
- **Sign-in URL:** `${WEB_URL}/sign-in?source=desktop`. The web page sends the browser to `/auth/callback?source=desktop` (web/src/pages/sign-in.tsx:31), which creates a one-time token through RPC `create_desktop_token` and opens `treq://auth/callback?token=<t>`.
- **Token exchange** (lines 91-127):
  - `POST ${SUPABASE_URL}/functions/v1/exchange-desktop-token`
  - Headers: `Content-Type: application/json`, `Authorization: Bearer <ANON_KEY>`, `apikey: <ANON_KEY>`. Body `{"token": t}`.
  - Success: `{access_token, refresh_token, expires_in, user}`.
  - Errors `{error}` with status 400 (missing), 401 ("Invalid or expired token" / "Token expired"), 404, 500. The client throws `err.error || "Token exchange failed"`.
  - Server: supabase/functions/exchange-desktop-token/index.ts.
- **setSession** (supabase-js behaviour to reimplement):
  - Validate the access token. If it has expired, refresh with `POST {url}/auth/v1/token?grant_type=refresh_token` (`apikey` header, body `{refresh_token}`).
  - Otherwise fetch the user with `GET {url}/auth/v1/user` (`Authorization: Bearer <access>`, `apikey`).
  - The session holds `access_token, refresh_token, expires_at (epoch seconds), user`.
- **Persistence:** `set_setting("supabase_session", JSON.stringify({accessToken, refreshToken}))`; `""` on sign-out (lines 39-51).
- **Restore** (lines 128-150): `get_setting("supabase_session")` → setSession → set user → fetch subscription. Always ends with `loading=false`.
- **Refresh:** nothing refreshes in the background. `getSession()` refreshes when the token is close to expiry (supabase-js margin, about 90 s in auth-js v2; check against the version in use). Callers: the 60 s token-sync tick, renewal's `isSessionValid`, and `functions.invoke`.
- **Subscription** (lines 70-87): `GET {url}/rest/v1/subscriptions?select=*` with `Accept: application/vnd.pgrst.object+json` (`.single()`), `apikey` and `Authorization: Bearer <access>` → `{status, plan, current_period_end}`. Failures are ignored.
- **Sign-out:** `POST {url}/auth/v1/logout` (supabase-js default scope global) with the Bearer access token. The `SIGNED_OUT` auth event also triggers `remote_cut_off_managed` (`remoteCutoffStore.ts:52-60`), and the token sync pushes `null` to `remote_set_relay_access_token`.

### 3.3 Control-plane Edge Functions (`lib/remote-control-plane.ts`)
- **Call shape:** `supabase.functions.invoke(name, {body:{action, ...}})` = `POST {url}/functions/v1/{name}` with `Authorization: Bearer <session access_token>` (the anon key if signed out), `apikey: <anon>`, `Content-Type: application/json`. supabase-js also sends `x-client-info`.
- **Error mapping** (lines 18-67), on non-2xx: read the JSON `{error, code?, provider_error?, correlation_id?}` and build `RemoteFunctionError{status, code}` with message `"[<code>] <error>\nHTTP <status> · Correlation ID: <id>"`. Code defaults to `provider_error` or `http_<status>`; correlation id falls back to the `x-correlation-id` header.
- **`remote-instance`** (server: remote-instance/index.ts:139-262):
  - `{action:"status"}` → `{instance: ManagedInstanceRecord|null, endpoint: {id, hostname, port, username, source}|null}`.
  - `ManagedInstanceRecord` fields: `instance_id, owner_user_id, provider_kind, provider_resource_id, region, size_preset, status, generation, endpoint_id, image_manifest_version, created_at, ready_at, disk_quota_gb, vcpu_quota, ram_quota_gb`.
  - `{action:"wake", instance_id, idempotency_key}` → `{operation_id, status}`.
  - Not used by mobile: `ensure`, `reprovision`, `delete`, `list_regions`, `list_sizes`.
  - 401 when the JWT is missing or invalid.
- **`remote-ssh-trust`** (server: remote-ssh-trust/index.ts):
  - `register_client_key {public_key, comment, idempotency_key}` (lines 182-230) → `{operation_id, status, key}`. An idempotent replay returns `{..., keys:[...]}`; the client then picks the key whose `comment` matches, else the first (`remote-control-plane.ts:147-161`).
  - `ClientKey` = `{id, algorithm, fingerprint_sha256, comment, created_at, revoked_at}`.
  - `issue_certificate {instance_id, key_id, renewal?: true}` (lines 305-429) → `{certificate, serial, expires_at (ISO), endpoint: SshEndpoint}`. The endpoint has `source {type:"managed", provider:"fly_sprites", generation}`, `authentication {type:"certificate", key_reference: <key id>}` (replaced locally) and `transport {type:"relay", url: <relay url with endpoint_id & key_id>}`.
  - Certificates last 20 minutes (line 67), ed25519 only.
  - Errors: 404 "Instance/Key/Endpoint does not belong to this user", 409 "Instance is not ready (status: X)" / "Key has been revoked" / "Instance has no endpoint recorded yet".
  - `report_cutoff {instance_id, endpoint_id, reason}` → `{status}`. Best-effort audit.
- **`remote-ssh-relay`** is already handled in Rust: a WebSocket to `endpoint.transport.url` with header `authorization: Bearer <supabase access token>` (core/remote_ssh_ws_stream.rs:154-164). Relay connections drop after a few minutes by design; the pool reconnects on the next command.

### 3.4 Relay access-token sync (`lib/remote-relay-auth.ts`, `lib/supabase-token-sync.ts`)
- Starts once per app run (`started` promise), on the first managed connect whose endpoint is relayed.
- It pushes `remote_set_relay_access_token(token)`:
  - on start (and the connect waits for that first push);
  - on every `onAuthStateChange` token change;
  - on a 60 s timer that calls `getSession()` (which may refresh).
- The same token is never pushed twice in a row. A failed push resets the de-dup so the next tick retries.
- In Rust, call `pool.set_relay_access_token(Some(tok)/None)`.

### 3.5 Certificate renewal and hard cutoff (`lib/remote-cert-lifecycle.ts`)
- **When to renew:** at `issuedAt + 0.8 × (expiresAt − issuedAt)` (`RENEWAL_REMAINING_LIFETIME_FRACTION = 0.2`; `renewalDelayMs`, lines 69-79). A delay of 0 means renew immediately. `issuedAt` is the client's clock.
- **Each attempt** (lines 212-251):
  1. `isSessionValid`: `getSession()` returns a session and `expires_at*1000 > now` (or 0). If not → cutoff `session_ended`.
  2. `issue_certificate {instance_id, key_id, renewal:true}`.
  3. On success, swap the endpoint's inline certificate. Keep the same endpoint id; the pool reuses open connections because auth is not part of `PoolKey` (transport.rs:676-693).
- **Error classification** (lines 110-125):
  - 401 → `session_ended`
  - 404 → `key_revoked` if the lower-cased message contains "key does not belong", else `instance_inaccessible`
  - 409 → `key_revoked` if it contains "revoked", else `instance_inaccessible`
  - anything else → retry
- **Retry backoff:** `min(120 s, 15 s × 2^n)`. If the time left is no more than the backoff, schedule cutoff `certificate_expired` at expiry instead.
- **On cutoff:** `remote_force_cutoff(endpointId, reason)` + `report_cutoff` (lines 294-303).

### 3.6 Settings keys stored on the device (SQLite via `get_setting`/`set_setting`)
| Key | Format |
|---|---|
| `supabase_session` | `{"accessToken","refreshToken"}` JSON, or `""` |
| `remote_user_managed_endpoints` | JSON array of `{id, display_name, hostname, port, username, host_key_fingerprint, auth_identity_reference, alias\|null, created_at}`; newest first, deduped by id (`remote-endpoints.ts:20,57-72`) |
| `remote_saved_repositories` | JSON array of `{id, endpoint_id, endpoint_generation, canonical_remote_path, display_name, last_successful_trust_validation\|null}` (`remote-endpoints.ts:21,36-45`; legacy field `remote_path` is read as `canonical_remote_path`) |
| `feature_preview.remoteSsh` | `"true"` / `"false"`, read by Rust |
| `lastRepoPath` | desktop preview only |

**Saved-repository upsert** (`remote-repository.ts:47-143`):
- `canonicalizeRemotePath`: trim; keep a `~` prefix; turn `\` into `/`; drop empty and `.` segments; resolve `..` by popping (never above the root).
- id = `remote-repo:${endpoint_id}:gen${gen}:${canonical}`. An existing duplicate with the same endpoint, generation and path is reused.
- `display_name` defaults to the canonical path.
- The upsert is refused if the JSON contains any of `private_key, privateKey, password, passphrase, credential(s), secret, ssh_key, sshKey`.
- The new record goes first in the list.

### 3.7 Mobile session snapshot (`lib/mobile-session.ts`)
- Stored in **WebView `localStorage`**, not SQLite, under key `"treq-mobile-session"`.
- JSON: `{endpoint: {kind:"managed"} | {kind:"user_managed", id}, repoPath: string|null, screen: RemoteRepoScreenState|null}`.
- `parseMobileSession` copies only known fields. `screen` is dropped when there is no `repoPath`, and `diff` requires a `path`.
- Cleared on "Switch host".
- The GPUI port needs its own store for this (a file, or a SQLite setting).

### 3.8 Device-key error state
- Errors starting with `secure_storage_unavailable:` lead to the dedicated alert (`useMobileRemoteConnection.ts:59,120-131`). The prefix must match Rust exactly (`remote_device_key.rs:40`).

---

## 4. Events the mobile frontend listens to

| Event | Payload | Listener | Emitted at |
|---|---|---|---|
| `remote://cutoff` | `{endpoint_id: string, reason: "session_ended"\|"key_revoked"\|"instance_inaccessible"\|"certificate_expired"}` | `stores/remoteCutoffStore.ts:19,62-114` (adds to the `cutoffs` map; started by `MobileShell.tsx:29-32`) | `commands/remote_control.rs:147-153` (`remote_force_cutoff`, `app.emit`) and lines 170-176 (`remote_cut_off_managed`, once per endpoint). Event name constant at line 124 |
| `remote-pty-data-<sessionId>` | `string` (UTF-8 decoded with a stream decoder that keeps multibyte characters split across chunks intact) | `api-extra.ts:179-185` → `RemoteTerminalPanel.tsx:208-211` → `xterm.write` | `commands/remote_pty_commands.rs:46-48,155-176` (`app.emit`), driven by the reader task in `core/remote_pty.rs:291-318` |
| `remote-pty-exit-<sessionId>` | `{exit_status: number\|null}` (exactly once; any trailing partial UTF-8 is flushed as data first) | `api-extra.ts:192-198` → `RemoteTerminalPanel.tsx:212-214` | `remote_pty_commands.rs:52-54,177-186` |
| deep links (plugin internal) | `string[]` URLs | `@tauri-apps/plugin-deep-link` `onOpenUrl` + `getCurrent` (`auth-deep-link.ts:47-48`) | Native code in tauri-plugin-deep-link on mobile. Not app code |
| `deep-link-received` (desktop only) | `string[]` | `AppStoreEffects.tsx:94-104` | `lib.rs:516-518`, `#[cfg(desktop)]` |
| Rust log forwarding | n/a | `initLogger` → `attachConsole` (lib/logger.ts) | tauri-plugin-log |

Also, renewal's `onCutoff` does **not** record the cutoff locally; it waits for the `remote://cutoff` event. The GPUI app can update its cutoff map directly when it calls `force_cutoff` or `cut_off_managed`.

---

## 5. What `run()` in lib.rs does on mobile

`#[cfg_attr(mobile, tauri::mobile_entry_point)] pub fn run()` (lib.rs:345-1041):

1. `telemetry::install_panic_hook()`. The CLI branch is desktop-only.
2. **Plugins** (lines 362-387): `tauri_plugin_log` (Info level, records forwarded to `telemetry::forward_log_record`), `opener`, `dialog`, `deep_link`, `notification`, and on mobile only `tauri_plugin_keystore::init()` + `tauri_plugin_biometric::init()`. `single_instance` and `cli` are desktop-only.
3. **`on_window_event` Destroyed** (lines 391-410): `pty_manager.close_all_for_window(label)` and `RemotePtyManager::close_all_for_window(label)` (remote_pty.rs:413).
4. **`setup`** (lines 411-499):
   - `telemetry::init(app_log_dir)` → `TelemetryGuards` (telemetry.rs:42).
   - Create the app data dir and **set the env var `TREQ_APP_DATA_DIR`** (line 420; used by `core::resolve_app_db_path` and `feature_preview::is_enabled_in_app_db`).
   - `Database::new(app_dir/treq.db)` (`core::APP_DB_FILE_NAME`) + `init()`.
   - Read `last_opened_repo_path` for the window URL and create the `main` WebviewWindow.
   - Binary-path and editor caches (`binary_paths::init_*`).
   - `PtyManager::new()`, `WatcherManager` (with AppHandle), `pr_status::set_app_handle`, `auto_review::set_emitter` (emits `auto-review-triggered`).
   - `agent_dispatch::bind_ephemeral_listener()`, then `AppState::new(...)` and `app.manage(app_state)`.
   - **Mobile:** `RemoteExecState(Arc::new(SshConnectionPool::new().with_device_key_provider(remote_device_key::device_key_provider(handle))))` (lines 487-492); desktop uses `RemoteExecState::default()`.
   - `app.manage(RemotePtyState::new(&exec))`, `app.manage(exec)`.
   - `start_agent_ipc_listener(handle, listener)`, `start_instance_registry_heartbeat(handle)` (private `agent_runtime`; desktop agent-dispatch features, not needed for the mobile remote flow).
   - The deep-link `on_open_url` handler and menus are `#[cfg(desktop)]`.
5. **`invoke_handler`**: the full command list (lines 805-1018).
6. **`RunEvent::Exit`** (lines 1031-1039): `pty_manager.close_all()` and `RemotePtyManager::close_all()` (remote_pty.rs:391).

**What a GPUI app needs to replicate for the mobile remote flow:**
- Run a tokio runtime.
- Choose the app data dir, set `TREQ_APP_DATA_DIR`, open `Database` and `init()`.
- Build an `Arc<SshConnectionPool>` with your own device-key provider, and a `RemotePtyManager` on the same pool. On exit, call `close_all()`.
- Optionally set up logging/telemetry (`telemetry::init` is pub).
- Handle deep links natively (Android intent filter / iOS URL type for `treq://auth/callback`) and open the browser natively in place of the opener plugin.
- Skip: PtyManager, WatcherManager, pr_status, auto_review, agent IPC, heartbeat, menus.

**Linking `treq_lib` as an rlib:**
- It still depends on `tauri`, its plugins and `tauri-build`. `build.rs` runs `tauri_build::build()`, which reads `src-tauri/tauri.conf.json`.
- `run()` contains `tauri::generate_context!()`, which needs `frontendDist ../src/dist` to exist at compile time (package.json has a `verify:frontend-dist` script for exactly this).
- Expect to need a built `src/dist`, or to feature-gate `run()`.

---

## 6. App identity and deep-link configuration

- **`src-tauri/tauri.conf.json`:**
  - `productName: "Treq"` (line 3), `version: "0.3.0"` (line 4), `identifier: "com.treq"` (line 5).
  - `build.frontendDist: "../src/dist"`, `devUrl http://localhost:1420`.
  - `app.windows: []`; the window is created in setup.
  - CSP `connect-src` allows `http://127.0.0.1:54321` and `https://xnlljmfiqyumiyexydyl.supabase.co`.
  - `plugins.deep-link` (lines 757-770):
    - mobile: `[{scheme:["treq"], host:"auth", pathPrefix:["/callback"], appLink:false}]`
    - desktop: `{schemes:["treq"]}`
  - `bundle.android.minSdkVersion: 28` (lines 781-782). There is no `iOS` bundle section.
- **`src-tauri/Info.ios.plist`:** `NSFaceIDUsageDescription` = "Treq uses Face ID to unlock the SSH key that connects this device to your computer." It is merged into `gen/apple/*_iOS/Info.plist`; `gen/` is not committed.
- **`src-tauri/capabilities/mobile.json`:** platforms android/iOS, window `main`. Permissions: `core:default`, `opener:allow-open-url` for `https://treq.dev/sign-in*` and `http://localhost:3001/sign-in*`, `log:default`, `deep-link:default`. Keystore and biometric are called only from Rust.
- **Deep-link format:** `treq://auth/callback?token=<one-time token>`. Rust's desktop classifier (`lib.rs:166-180`) and TS `authCallbackToken` require scheme `treq`, host `auth`, path exactly `/callback`.
- **npm mobile scripts:** `mobile:android:{init,dev,build}` and `mobile:ios:{init,dev,build}` (package.json:47-52).
- Product requirements and acceptance criteria are in `prds/mobile.md`; the code comments cite criteria 7 (restore from remote identifiers) and 8 (block on host-trust or authorization failure).

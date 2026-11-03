# Projects: multi-repo work in Treq

Status: draft spec, no implementation. Date: 2026-11-03.

## Decisions taken with the owner

- **Synced from day one** via Supabase.
- **Personal only**: a project belongs to one user; no team sharing.
- **Project is the top-level entity; repos are many-to-many with projects**: a repo is added to one or more projects (like Claude Code projects). The repo itself is not owned by any one project: its checkout, `.treq` state and workspaces are shared, and each project adds its own grouping, prompts and context on top. A single-repo user gets an implicit one-repo project.

## Relationship to supporting repositories

Projects are **v2 of supporting repositories** and replace them. The current feature (v1):

- `supporting_repos(repo_path, added_at)` in the home repo's `.treq` DB (`local_db::list_supporting_repo_paths`, `add_supporting_repo_path`); linked via **File > Add Repository…**.
- CLI `-r <repo>` on `add`, `set`, `st`, `diff`, `agent`, `commit` (`--repo` on `mv`) to act on a linked repo.
- `agent_command::system_prompt()` lists supporting repos; `claude_filesystem_settings()` grants read on them. The agent is told to write only via `treq add -r` + `treq agent -r`, and to check with `treq st -r`.

What v1 lacks and Projects add:

| v1 supporting repos | v2 projects |
|---|---|
| Links are local to one home repo's DB, one direction | Project is a synced, first-class group; repos are many-to-many |
| No shared prompts or context | Project prompts and context (D9–D12) |
| Sub-agents are fire-and-forget, checked one at a time with `st -r` | Orchestration: the home agent spawns, tracks and reads all sub-agents' workspaces in one call |
| No cross-repo grouping of work | Project workspaces, shared branch, grouped PRs |
| Repo identity = local path | `repo_key`, synced, path per machine |

**Superseding plan**: keep the `-r` flag and the agent's read-only rule; resolve `-r` against the project's repos instead of `supporting_repos`. Migrate each home repo with supporting repos into one project (home repo + its supporting repos), then stop reading `supporting_repos` (table kept one release for rollback, then dropped). The UI's **Add Repository…** becomes "Add repository to project".

## Current model (what this builds on)

- Repo = a path on disk. Per-repo state lives in `<repo>/.treq/` (`local_db.rs`: `workspaces`, `sessions`, reviews, stashes; `.treq/config.yaml` via `repo_config.rs`).
- App-level state: `db.rs` (`settings`, repo-keyed settings, `git_cache`, `file_views`).
- Workspace = a jj workspace under `.treq/workspaces/<name>` with bookmark `treq/{name}`.
- Sync: `user_config_snapshots` (migration 050) stores app and `repo:%` scoped config, owner-only via security-definer RPCs.

## Problem

Work that spans repos (API + client, service + infra, lib + consumer) is done as N unrelated workspaces. Nothing ties their branches, PRs, agents or status together, and the repo list is flat with no grouping.

## Goals

- Group repos under a named project; the project is the unit the sidebar, launcher and CLI open.
- Create one *project workspace* that creates a workspace per selected repo with one shared branch name.
- Run agents/sessions scoped to a project workspace (cwd = a project root holding each repo's workspace).
- Show linked status: per-repo diff/conflict/check/PR state rolled up.
- Open and track a project workspace's PRs across repos, inside Treq.
- Sync the project definition across the user's machines.

## Non-goals

- Team sharing, roles, invites.
- Atomic cross-repo commits or merges (git has none; we coordinate, not guarantee).
- Monorepo splitting, submodule or `repo`-tool management.
- Changing how a single repo's workspaces work internally.

## User stories

1. Create a project, name it, add repos from disk or by cloning a remote URL.
2. Edit name/icon/default branch pattern; add/remove repos (removal never deletes files).
3. Open a project: sidebar lists its repos and project workspaces.
4. Start "feature X" across repos A and B: one action creates `treq/feature-x` in both.
5. Launch one agent with all repos of the workspace in view, or one agent per repo.
6. Commit per repo; push all; open one PR per repo, grouped in Treq.
7. See one status row: "A: 2 ahead, PR open, checks green · B: conflict".
8. On a second machine, the project appears; repos not cloned there show "Clone" / "Locate".

## Key decisions

### D1. Where the project definition lives
- A. App-level SQLite only.
- B. Supabase as source of truth, app SQLite as cache. **(Recommended, matches "synced day one")**
- C. Checked-in manifest file (`treq-project.yaml`) in one repo.

B keeps offline working (cache) and needs no repo to own the project. A fails the sync requirement; C couples a personal concept to a shared repo.

### D2. Repo identity
- A. Local path.
- B. Normalised remote URL (`github.com/owner/name`), path stored per machine. **(Recommended)**
- C. GitHub repo node ID.

Paths differ per machine; B syncs and works for non-GitHub remotes. Store GitHub node ID as optional metadata to survive renames (see edge cases). Repos with no remote get `local:<uuid>` identity and are flagged "this machine only".

### D3. Shape of a project workspace on disk
- A. No shared folder; each repo keeps its own `.treq/workspaces/<name>`, project workspace is a DB row linking them.
- B. A. plus a project root `~/.treq/projects/<slug>/<ws>/` holding symlinks to each repo workspace. **(Recommended)**
- C. Clone repos fresh into a project folder.

B gives agents one cwd that sees every repo, without moving repo state. C duplicates clones.

### D4. Branch naming across repos
- A. Identical bookmark in every repo, from the project pattern (default `treq/{name}`). **(Recommended)**
- B. Per-repo pattern from each `.treq/config.yaml`.
- C. Free-form per repo.

A makes linking trivial (match by name). Per-repo override allowed only as explicit opt-out (D6).

### D5. Coordinated PRs
- A. Independent PRs, grouped only inside Treq (matched by project workspace branch); PR descriptions untouched. **(Recommended)**
- B. Independent PRs, each body carries a managed "Linked PRs" block, kept in sync by Treq.
- C. Merge-train / ordered merge enforcement.

A keeps PR descriptions owned by the author and needs no sync of PR bodies. B adds noise reviewers did not ask for. C is a later phase: optional "merge order" hint, no enforcement.

### D6. Config overrides
- A. Project config only.
- B. Repo `.treq/config.yaml` wins over project defaults, per key. **(Recommended)**
- C. Project wins over repo.

Repo config is checked in and shared with teammates; a personal project must not override it. Project config adds defaults (pattern, agent, model) and project-only keys (agent scope).

### D7a. Repo membership
- A. A repo belongs to exactly one project.
- B. Many-to-many: a repo can be in one or more projects, sharing one checkout. **(Recommended, matches Claude Code projects)**
- C. Many-to-many with a separate checkout per project.

B lets "payments" and "platform" both include the same shared library without duplicate clones or diverging state. C wastes disk and splits workspaces.

### D7. Sessions/agents scope
- A. Session belongs to one repo workspace (today).
- B. Session belongs to a project workspace, cwd = project root (D3); optional per-repo sessions too. **(Recommended)**
- C. Only project-level sessions.

### D8. Migrating existing users
- A. Auto-create one project per repo. **(Recommended)**
- B. One "Default" project containing all repos.
- C. Prompt on first launch.

A preserves today's mental model; the user merges projects later.

## Data model

### Supabase (migration `0NN_projects.sql`)

```sql
create table public.projects (
  id uuid primary key default gen_random_uuid(),
  owner_user_id uuid not null references auth.users(id) on delete cascade,
  name text not null check (length(name) between 1 and 100),
  slug text not null,
  home_repo_key text,                     -- orchestrator repo (D13)
  config jsonb not null default '{}',     -- branch pattern, default agent/model
  version bigint not null default 1,      -- optimistic concurrency
  deleted_at timestamptz,                 -- soft delete, tombstone for sync
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique (owner_user_id, slug)
);

create table public.project_repos (
  project_id uuid not null references public.projects(id) on delete cascade,
  repo_key text not null,                 -- normalised remote URL or local:<uuid>
  github_node_id text,
  display_name text,
  position int not null default 0,
  overrides jsonb not null default '{}',  -- e.g. {"branch_pattern": ...}
  primary key (project_id, repo_key)
);

create table public.project_workspaces (
  id uuid primary key default gen_random_uuid(),
  project_id uuid not null references public.projects(id) on delete cascade,
  name text not null,
  branch_name text not null,
  repo_keys text[] not null,
  archived boolean not null default false,
  created_at timestamptz not null default now(),
  unique (project_id, name)
);
```

RLS: enable on all three; `revoke all from anon`. Policy: `owner_user_id = (select auth.uid())` on `projects`; child tables check `exists (select 1 from projects p where p.id = project_id and p.owner_user_id = (select auth.uid()))`. Writes go through security-definer RPCs (`put_project(p_id, p_expected_version, ...)`, `put_project_prompt`, `put_project_context_ref`) like `put_user_config`, so version conflicts raise `version_conflict`. Never store local paths server-side.

### Local (app-level `db.rs`)

- `projects_cache`, `project_repos_cache`, `project_workspaces_cache`: mirrors + `sync_state` (`clean|dirty|conflict`).
- `repo_locations(repo_key, machine_path)`: per-machine path map. Never synced.
- Per-repo `.treq` DB: add nullable `workspaces.project_id TEXT`, `workspaces.project_workspace_id TEXT`, `sessions.project_id TEXT`, `sessions.project_workspace_id TEXT`. `project_id` records the project a workspace or session was created from, which decides its prompts and context. Repo DBs stay authoritative for workspace state.

### Migration

- Local DB migration adds columns (tested with an upgrade fixture in `local_db/upgrade_fixtures`).
- First launch after upgrade: for each known repo, create a one-repo project (D8); push when signed in, stay `dirty` while offline/signed out.
- Signed-out users: projects work locally; sync on sign-in, merge by `repo_key`.

## UX (GPUI shell)

- **Sidebar**: project switcher at top; under it, repos (collapsible) and project workspaces (badge: N repos).
- **New project**: modal → name → add repos (pick folder / clone URL / recent repos) → done.
- **Project settings** (settings screen, new "Project" section): name, branch pattern, default agent/model, repo list with reorder/remove, per-repo overrides.
- **New project workspace**: name → checkboxes of repos (default all) → creates workspaces in parallel; partial failure shows per-repo result with retry, never rolls back successful ones silently.
- **Project workspace screen**: tab per repo reusing existing changes/commits screens, plus a "Summary" tab with the rolled-up status table and "Push all" / "Open PRs".
- **Missing repo on this machine**: row greyed with "Clone" / "Locate…".

## CLI / API

```
treq project list|create <name>|rename|delete
treq project add-repo <project> <path|url>
treq project remove-repo <project> <repo>
treq project ws create <project> <name> [--repos a,b]
treq project ws status <project> <name> [--json]
treq project ws push <project> <name>
treq project ws pr <project> <name> [--draft]
```

- Handlers in `cli/project_handlers.rs`, thin; logic in `core/projects/`. Tauri `commands/projects.rs` equally thin.
- Supabase RPCs: `put_project`, `get_projects(since)`, `delete_project`, `put_project_workspace`.

## Cross-repo semantics

- **Branch naming**: one bookmark per project workspace (D4). Create fails fast if the bookmark exists in any repo, listing which.
- **Commits**: per repo. "Commit all" with one message is a convenience that runs per-repo commits; reported per repo, not atomic.
- **PRs**: open per repo with the repo's normal title/body; Treq groups them in the Summary tab by project workspace (uses existing `pr_status.rs`). Treq does not edit PR descriptions.
- **Linked status**: summary = worst of per-repo states (conflict > failing checks > behind > clean).
- **Rebase**: existing auto-rebase runs per repo; no cross-repo ordering.
- **Overrides**: resolution order repo config.yaml > project repo override > project config > app default (D6).

## Project prompts and context

A project carries **common prompts** and **context references** that Treq adds to every terminal agent it launches in any of the project's repos: per-repo workspaces, the home repo, and project workspaces.

### Today's injection point

`core/agent_command.rs::system_prompt()` builds the Treq system prompt; `prepare_auto_command()` writes it to a temp file passed as `--append-system-prompt-file` (Claude) or prepended to the prompt file (Cursor). Claude filesystem scope comes from `claude_filesystem_settings()`. Project prompts and context plug in here; nothing else in the launch path changes.

### Model

- **Prompt**: `{id, title, body (markdown), enabled, position, applies_to: all | repo_keys[]}`.
- **Context ref**: `{id, kind: file | dir | url, target, label, applies_to}`.
  - `url`: synced as is.
  - `file`/`dir` inside a project repo: stored as `{repo_key, relative_path}`, so it syncs and resolves on every machine.
  - `file`/`dir` elsewhere on disk: stored per machine (local table, like `repo_locations`), never synced. Shown as "this machine only".

### Decisions

**D9. How prompts reach the agent**
- A. Append to the Treq system prompt file. **(Recommended)**
- B. Write a generated `CLAUDE.md`/`AGENTS.md` into each workspace.
- C. Send as the first user message.

A reuses the existing file and flag, leaves the repo tree untouched (B would show up in diffs and fight checked-in agent files), and does not take up the user's first turn (C).

**D10. How context files reach the agent**
- A. List resolved paths/URLs in the system prompt and grant read access to them. **(Recommended)**
- B. Inline file contents into the prompt.
- C. Copy files into the workspace.

A keeps prompts small and current; the agent reads on demand. Read access: Claude gets `--add-dir` for each dir and read allow rules in the generated settings; the scope text in `system_prompt()` gains "You may read these project context paths: …" so the existing "do not read outside this directory" rule has an explicit exception. Read only; never write. B is an opt-in per file (`inline: true`, capped at 32 KiB) for short files such as a style guide. C goes stale.

**D11. Layering with repo and app prompts**
- A. Order: Treq base → project prompts → repo prompt (if added later) → task prompt; all concatenated. **(Recommended)**
- B. Project replaces repo.
- C. User-chosen order per prompt.

A follows D6: more specific comes later, so it reads as an override to the model, and nothing is silently dropped.

**D12. Where prompts are stored**
- A. Rows in Supabase (`project_prompts`, `project_context_refs`). **(Recommended)**
- B. Inside `projects.config` jsonb.
- C. Markdown files in a chosen repo.

A allows per-item edits without whole-project version conflicts and keeps the `projects` row small. C couples a personal concept to a shared repo (non-goal: team sharing).

### Schema addition

```sql
create table public.project_prompts (
  id uuid primary key default gen_random_uuid(),
  project_id uuid not null references public.projects(id) on delete cascade,
  title text not null,
  body text not null check (octet_length(body) <= 32768),
  enabled boolean not null default true,
  position int not null default 0,
  repo_keys text[],                      -- null = all repos
  updated_at timestamptz not null default now()
);

create table public.project_context_refs (
  id uuid primary key default gen_random_uuid(),
  project_id uuid not null references public.projects(id) on delete cascade,
  kind text not null check (kind in ('file','dir','url')),
  repo_key text,                         -- set for repo-relative refs
  relative_path text,
  url text,
  label text,
  inline boolean not null default false,
  repo_keys text[],
  check ((kind = 'url') = (url is not null)),
  check ((kind = 'url') or (repo_key is not null and relative_path is not null))
);
```

Same owner RLS as other child tables. Local: `project_context_local(id, project_id, kind, machine_path, label, inline, repo_keys)`.

### UX and CLI

- Project settings → "Agent context" section: prompt list (add, edit in a markdown editor, reorder, toggle, scope to repos) and context list (add file/dir via picker, add URL).
- Agent launch shows a "Project context: 3 prompts, 4 files" chip; clicking previews the final system prompt.
- Agents already running keep their prompt; edits apply to the next launch (stated in the UI).

```
treq project prompt list|add|edit|rm|toggle <project> ...
treq project context add <project> <path|url> [--inline] [--repos a,b]
treq project context list|rm <project> ...
treq project prompt preview <project> [--repo r]   # prints the final system prompt
```

### Agent orchestration across project repos

A terminal agent in the project's **home repo** is the orchestrator. It can start several terminal agents across the project's repos, each in its own workspace, stacked where needed, and read the state of every workspace in the project.

### Home repo

**D13. What the orchestrator runs in**
- A. A designated home repo per project (`projects.home_repo_key`), defaulting to the first repo added. **(Recommended)**
- B. The project root folder (D3) with no repo of its own.
- C. Any repo in the project.

A matches v1 (the agent starts in a real repo, with its own `AGENTS.md`/skills) and gives `treq` a repo to resolve against. B has no git state to commit plans or notes into. C makes "which project context applies" ambiguous for repos in several projects. Per-machine override allowed if the home repo is not cloned locally.

### Spawning sub-agents

- Same commands as v1, resolved against the project: `treq add -r <repo> <branch> [-s <parent>]` then `treq agent -r <repo> <branch> "<prompt>"`.
- **Stacking**: `-s/--source-branch` stacks a workspace on another workspace's branch in the same repo, as today. Stacks never cross repos; cross-repo ordering is expressed as a dependency note in the workspace description, not as a jj parent.
- One call for fan-out: `treq project spawn --plan plan.json` (or stdin) creates N workspaces and agents. Plan entries: `{repo, branch, source_branch?, prompt, mode?}`. Each entry succeeds or fails on its own; the result lists per-entry status.
- Every sub-agent gets the project's prompts and context (D11), plus a short parent note: "Started by the orchestrator in <home repo> for project <name>; report progress with `treq set -d`."
- Sub-agents record `sessions.parent_session_id` (new, nullable) so the UI shows the tree and the orchestrator can list its own children.

**D14. Can sub-agents spawn agents?**
- A. No: only the home-repo agent orchestrates. **(Recommended for v2)**
- B. Yes, to a depth limit.
- C. Yes, unlimited.

A keeps cost and blast radius visible and the session tree one level deep. B can follow once usage shows a need.

### Reading project workspaces

- `treq project st [--repo r] [--json]`: every workspace in every project repo: repo, branch, stack parent/children, uncommitted/conflict/commit counts, PR state, agent state (running, waiting for input, exited), description. Same fields as `treq st`, one call.
- `treq st -r <repo> <branch>` and `treq diff -r <repo> <branch>` keep working for detail.
- Any agent in the project (not only the orchestrator) may run these reads; they are read-only.
- Agent state comes from `agent_supervisor` / `pty_daemon`; workspace state from each repo's `.treq` DB and jj. Repos missing on this machine show as unavailable, not an error.

**D15. How agents learn about sibling progress**
- A. Pull: agents run `treq project st` when they need it. **(Recommended)**
- B. Push: Treq sends a message into the orchestrator's terminal when a sub-agent exits or asks for input (existing `treq send`/notify path).
- C. Shared log file all agents append to.

A is simple and works for every agent CLI. B is a Phase-3 addition behind a setting, reusing `send_dispatch`; it must never type into a terminal that is mid-input. C goes stale and conflicts.

### Permissions

- Orchestrator: write only in its own cwd (home repo or its workspace), read on all project repos (v1 rule, now over project repos). Writes elsewhere only through sub-agents.
- Sub-agent: write in its own workspace; read on other project repos.
- `claude_filesystem_settings()` takes the project repo list instead of `supporting_repo_paths`.
- Limits: per-project cap on concurrent agents (default 8, in project config); `spawn` refuses beyond it.

### UI

- Orchestrator session shows a child list (repo, branch, agent state) with jump-to-terminal.
- Sidebar marks workspaces started by an orchestrator.


- Context path missing on this machine: skipped, listed as "unavailable" in the preview, agent still launches.
- Repo-relative ref to a repo not cloned here: skipped with the same warning.
- Context path that contains secrets (`.env`, keys): refuse to add paths matching a deny list; warn on dirs that contain them.
- Huge prompts: total injected text capped (64 KiB); launch warns and truncates lowest-priority prompts last-first.
- Same repo in two projects: an agent uses the project its session was opened from (`sessions.project_id`, else the workspace's `project_id`, else the project active in the sidebar). Never merges two projects' prompts.
- Non-Claude agents without `--add-dir`: paths are listed in the prompt only; the preview says read access is not enforced for that agent.

## Edge cases

- **Same repo in multiple projects**: the normal case (many-to-many through `project_repos`). One checkout per machine, shared by all its projects; repo state stays in the repo's `.treq`. Each project lists all of the repo's workspaces, tagged with the project they were created from; a filter shows "this project only". A workspace belongs to at most one project workspace. Removing a repo from one project does not touch others; removing it from its last project asks whether to remove it from Treq (files are never deleted).
- **Missing access** (no clone, no GitHub permission, auth expired): repo shown as unavailable; project workspace ops skip it with a visible warning; the Summary tab omits PRs we cannot read.
- **Renamed repo**: match on `github_node_id`, update `repo_key`; local detection via remote URL change on fetch.
- **Deleted repo / missing path**: mark unavailable, keep row; user removes explicitly. `jj.rs` path-validating functions already return defaults on missing paths.
- **Monorepo**: a project with one repo is valid. Optional `sparse_patterns` per project repo (workspaces already support them) to scope a project to subdirs; no virtual "sub-repos".
- **Local-only repo (no remote)**: syncs as `local:<uuid>`, shown "available on <machine> only".
- **Sync conflicts**: version mismatch → refetch, field-level merge for repo list (union adds, honour tombstones), last-write-wins for name/config with a toast.
- **Repo path moved on disk**: "Locate…" updates `repo_locations`.

## Phased rollout

1. **Model + sync**: Supabase tables/RLS/RPCs, local cache, auto-migration (one project per repo), sidebar switcher. Behind `projects` flag.
2. **Supersede supporting repos**: migrate `supporting_repos` to projects, resolve `-r` against project repos, `treq project st`.
3. **Project prompts and context** (D9–D12): injection in `system_prompt()`, settings section, CLI.
4. **Project workspaces**: shared-branch creation, project root symlinks, project-scoped sessions/agents, summary tab, CLI.
5. **Grouped PRs + push all**, rolled-up checks.
6. **Orchestration**: `treq project spawn`, `parent_session_id`, child list UI, concurrency cap, optional push notifications (D15 B).
7. **Polish**: merge-order hint, sparse monorepo scoping, remove flag.

## Test strategy (TDD)

Each step starts with a failing test.

- `core/projects/` unit tests: repo_key normalisation (ssh/https/.git suffix), override resolution order, branch-name collision detection, status roll-up, PR grouping by project workspace, sync merge rules. Proptests for normalisation and merge.
- Prompt assembly: `system_prompt()` with project prompts in D11 order, repo scoping, disabled prompts skipped, missing context path skipped with warning, size cap, Claude settings/`--add-dir` include context dirs read-only, Cursor prompt file includes them; extend the existing tests in `agent_command.rs`.
- Supporting-repos migration: fixture DB with `supporting_repos` → one project with the same repos; `-r` resolves the same names before and after (extend `cli/tests.rs` `-r` cases).
- Orchestration: `spawn` plan parsing and per-entry partial failure; concurrency cap; sub-agent spawn refused (D14); `project st --json` shape across 2 temp repos with stacked workspaces; filesystem settings use project repos.
- `local_db` migration tests with upgrade fixtures for new columns; `db.rs` cache tables.
- Integration (`src-tauri/tests/`): create project workspace across 2 temp jj repos; partial failure leaves successful repos intact; missing path returns defaults.
- CLI tests in `cli/tests.rs` for each subcommand, `--json` shape snapshot.
- GPUI `tests.rs`: sidebar switcher, new-project modal, unavailable-repo row, summary tab; QA with `app-qa`.
- Supabase: RLS tests (other user cannot read/write; anon denied), `version_conflict`, tombstones, via `service-qa` against the local stack; `npm test` for any Edge Function.
- Bench: status roll-up for 10 repos × 20 workspaces under `src-tauri/benches/`.

## Open questions

- Should projects also rename repo-scoped settings keys in `db.rs`, or keep repo keys and add project keys?
- Project root symlinks on Windows (junctions vs. copy-free alternative)?
- Do mobile/remote (`mobile/`, `remote/`) surfaces need projects in Phase 1?

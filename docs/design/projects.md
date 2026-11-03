# Projects: multi-repo work in Treq

Status: draft spec, no implementation. Date: 2026-11-03.

## Decisions taken with the owner

- **Synced from day one** via Supabase.
- **Personal only**: a project belongs to one user; no team sharing.
- **Project replaces repo as the top-level entity**: every repo lives inside a project. A single-repo user gets an implicit one-repo project.

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
- Open and track linked PRs across repos.
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
6. Commit per repo; push all; open linked PRs that reference each other.
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
- A. Independent PRs, no linking.
- B. Independent PRs, each body carries a managed "Linked PRs" block, kept in sync by Treq. **(Recommended)**
- C. Merge-train / ordered merge enforcement.

B is cheap and visible on GitHub. C is a later phase: optional "merge order" hint, no enforcement.

### D6. Config overrides
- A. Project config only.
- B. Repo `.treq/config.yaml` wins over project defaults, per key. **(Recommended)**
- C. Project wins over repo.

Repo config is checked in and shared with teammates; a personal project must not override it. Project config adds defaults (pattern, agent, model) and project-only keys (agent scope, PR linking).

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
  config jsonb not null default '{}',     -- branch pattern, default agent/model, pr_linking
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

RLS: enable on all three; `revoke all from anon`. Policy: `owner_user_id = (select auth.uid())` on `projects`; child tables check `exists (select 1 from projects p where p.id = project_id and p.owner_user_id = (select auth.uid()))`. Writes go through security-definer RPCs (`put_project(p_id, p_expected_version, ...)`) like `put_user_config`, so version conflicts raise `version_conflict`. Never store local paths server-side.

### Local (app-level `db.rs`)

- `projects_cache`, `project_repos_cache`, `project_workspaces_cache`: mirrors + `sync_state` (`clean|dirty|conflict`).
- `repo_locations(repo_key, machine_path)`: per-machine path map. Never synced.
- Per-repo `.treq` DB: add nullable `workspaces.project_workspace_id TEXT`, `sessions.project_workspace_id TEXT`. Repo DBs stay authoritative for workspace state.

### Migration

- Local DB migration adds columns (tested with an upgrade fixture in `local_db/upgrade_fixtures`).
- First launch after upgrade: for each known repo, create a one-repo project (D8); push when signed in, stay `dirty` while offline/signed out.
- Signed-out users: projects work locally; sync on sign-in, merge by `repo_key`.

## UX (GPUI shell)

- **Sidebar**: project switcher at top; under it, repos (collapsible) and project workspaces (badge: N repos).
- **New project**: modal → name → add repos (pick folder / clone URL / recent repos) → done.
- **Project settings** (settings screen, new "Project" section): name, branch pattern, default agent/model, PR linking toggle, repo list with reorder/remove, per-repo overrides.
- **New project workspace**: name → checkboxes of repos (default all) → creates workspaces in parallel; partial failure shows per-repo result with retry, never rolls back successful ones silently.
- **Project workspace screen**: tab per repo reusing existing changes/commits screens, plus a "Summary" tab with the rolled-up status table and "Push all" / "Open linked PRs".
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
- **PRs**: open per repo; managed `<!-- treq:linked-prs -->` block lists siblings; refreshed when any sibling PR is created/closed (uses existing `pr_status.rs`).
- **Linked status**: summary = worst of per-repo states (conflict > failing checks > behind > clean).
- **Rebase**: existing auto-rebase runs per repo; no cross-repo ordering.
- **Overrides**: resolution order repo config.yaml > project repo override > project config > app default (D6).

## Edge cases

- **Same repo in multiple projects**: allowed; repo state stays in the repo's `.treq`. A workspace belongs to at most one project workspace. Removing a repo from one project does not touch others.
- **Missing access** (no clone, no GitHub permission, auth expired): repo shown as unavailable; project workspace ops skip it with a visible warning; PR linking omits PRs we cannot read.
- **Renamed repo**: match on `github_node_id`, update `repo_key`; local detection via remote URL change on fetch.
- **Deleted repo / missing path**: mark unavailable, keep row; user removes explicitly. `jj.rs` path-validating functions already return defaults on missing paths.
- **Monorepo**: a project with one repo is valid. Optional `sparse_patterns` per project repo (workspaces already support them) to scope a project to subdirs; no virtual "sub-repos".
- **Local-only repo (no remote)**: syncs as `local:<uuid>`, shown "available on <machine> only".
- **Sync conflicts**: version mismatch → refetch, field-level merge for repo list (union adds, honour tombstones), last-write-wins for name/config with a toast.
- **Repo path moved on disk**: "Locate…" updates `repo_locations`.

## Phased rollout

1. **Model + sync**: Supabase tables/RLS/RPCs, local cache, auto-migration (one project per repo), sidebar switcher. Behind `projects` flag.
2. **Project workspaces**: shared-branch creation, project root symlinks, project-scoped sessions/agents, summary tab, CLI.
3. **Linked PRs + push all**, rolled-up checks.
4. **Polish**: merge-order hint, sparse monorepo scoping, remove flag.

## Test strategy (TDD)

Each step starts with a failing test.

- `core/projects/` unit tests: repo_key normalisation (ssh/https/.git suffix), override resolution order, branch-name collision detection, status roll-up, linked-PR block render/parse (idempotent), sync merge rules. Proptests for normalisation and merge.
- `local_db` migration tests with upgrade fixtures for new columns; `db.rs` cache tables.
- Integration (`src-tauri/tests/`): create project workspace across 2 temp jj repos; partial failure leaves successful repos intact; missing path returns defaults.
- CLI tests in `cli/tests.rs` for each subcommand, `--json` shape snapshot.
- GPUI `tests.rs`: sidebar switcher, new-project modal, unavailable-repo row, summary tab; QA with `app-qa`.
- Supabase: RLS tests (other user cannot read/write; anon denied), `version_conflict`, tombstones, via `service-qa` against the local stack; `npm test` for any Edge Function.
- Bench: status roll-up for 10 repos × 20 workspaces under `src-tauri/benches/`.

## Open questions

- Should "project replaces repo" also rename repo-scoped settings keys in `db.rs`, or keep repo keys and add project keys?
- Project root symlinks on Windows (junctions vs. copy-free alternative)?
- Do mobile/remote (`mobile/`, `remote/`) surfaces need projects in Phase 1?

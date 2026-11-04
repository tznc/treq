# Projects: multi-repo work in Treq

Status: draft spec, no implementation. Date: 2026-11-03.

## Decisions taken with the owner

- **Synced from day one** via Supabase.
- **Treq manages clones**: users never clone by hand. Adding a repo by URL, or an agent touching a project repo that is not on this machine, makes Treq clone it to a managed location, register it, and create and link workspaces. Agents drive this through the CLI.
- **Personal only**: a project belongs to one user; no team sharing.
- **Repos stay standalone; projects are opt-in**: users open a repo directly, as today, with no project. A project is created and named only when the user adds a second repo to work on alongside the first. Repos are many-to-many with projects (like Claude Code projects): a repo can be in zero, one or more projects. The repo itself is not owned by any project: its checkout, `.treq` state and workspaces are shared, and each project adds its own grouping, prompts and context on top.

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

**Superseding plan**: keep the `-r` flag and the agent's read-only rule; resolve `-r` against the project's repos instead of `supporting_repos`. Migrate each home repo with supporting repos into one project (home repo + its supporting repos), then stop reading `supporting_repos` (table kept one release for rollback, then dropped). **Add Repository…** on a standalone repo now creates a project (see "Creating a project" under UX); on a repo opened inside a project it adds to that project.

## Current model (what this builds on)

- Repo = a path on disk. Per-repo state lives in `<repo>/.treq/` (`local_db.rs`: `workspaces`, `sessions`, reviews, stashes; `.treq/config.yaml` via `repo_config.rs`).
- App-level state: `db.rs` (`settings`, repo-keyed settings, `git_cache`, `file_views`).
- Workspace = a jj workspace under `.treq/workspaces/<name>` with bookmark `treq/{name}`.
- Sync: `user_config_snapshots` (migration 050) stores app and `repo:%` scoped config, owner-only via security-definer RPCs.

## Problem

Work that spans repos (API + client, service + infra, lib + consumer) is done as N unrelated workspaces. Nothing ties their branches, PRs, agents or status together, and the repo list is flat with no grouping.

## Goals

- Keep opening a single repo exactly as today; no project needed.
- When work needs a second repo, create a named project in that same flow; the project then becomes the unit the sidebar, launcher and CLI open.
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
8. On a second machine, the project appears; repos not yet on that machine are cloned by Treq in the background the first time anything (the user, the orchestrator, a sub-agent) needs them.
9. The orchestrator agent adds a repo it needs by URL (`treq project add-repo <url>`); Treq clones it, adds it to the project and the agent can start working in it straight away.

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
- B. Many-to-many: a repo can be standalone or in one or more projects, sharing one checkout. **(Recommended, matches Claude Code projects)**
- C. Many-to-many with a separate checkout per project.

B lets "payments" and "platform" both include the same shared library without duplicate clones or diverging state. C wastes disk and splits workspaces.

### D7. Sessions/agents scope
- A. Session belongs to one repo workspace (today).
- B. Session belongs to a project workspace, cwd = project root (D3); optional per-repo sessions too. **(Recommended)**
- C. Only project-level sessions.

### D8. Migrating existing users
- A. No change for plain repos; only home repos with `supporting_repos` become a project (home repo + its supporting repos, named after the home repo, renameable). **(Recommended)**
- B. Auto-create one project per repo.
- C. One "Default" project containing all repos.

A keeps standalone repos exactly as they are and creates projects only where v1 already grouped repos. B and C force a concept on users who work in one repo.

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
- First launch after upgrade: for each home repo with `supporting_repos` rows, create a project (D8); other repos stay standalone. Push when signed in, stay `dirty` while offline/signed out.
- Signed-out users: projects work locally; sync on sign-in, merge by `repo_key`.

## UX (GPUI shell)

- **Opening**: the launcher lists recent repos and projects side by side. Opening a repo works as today; no project UI appears for a standalone repo beyond **Add Repository…**.
- **Creating a project**: from a standalone repo, **Add Repository…** → pick folder / paste a repo URL or `owner/name` / recent repo (Treq clones URLs itself) → "Name this project" (prefilled with the current repo's name) → done. The current repo becomes the home repo (D13). A project can also be created empty from the launcher, but this is the main path.
- **Sidebar**: all active projects at once, as a three-level tree: project › repository › workspace. Clicking a project opens its project home; a repository row is a group header (click opens the repo-scoped view, chevron collapses); clicking a workspace opens it in its repo. There is no separate repo list; standalone repos keep their own section below the projects. Opening a repo card on project home expands, highlights and scrolls to that repo group. A repo in two projects appears under both.
- **Project home** (project with 2+ repos): replaces the single-repo home screen. Header shows the project title. Same three tabs as today's workspace screen (`gpui_shell/screen/workspace.rs`: Code, Changes, Commits), each project-wide, each drilling into today's repo-scoped screen. Detailed below.
- **Project settings** (settings screen, new "Project" section): name, branch pattern, default agent/model, repo list with reorder/remove, per-repo overrides.
- **New project workspace**: name → checkboxes of repos (default all) → creates workspaces in parallel; partial failure shows per-repo result with retry, never rolls back successful ones silently.
- **Project workspace screen**: tab per repo reusing existing changes/commits screens, plus a "Summary" tab with the rolled-up status table and "Push all" / "Open PRs".
- **Repo not yet on this machine**: shown with a "Not on this machine" state and no action needed; Treq clones it on first use (opening it, creating a workspace, an agent spawn) and shows progress ("Cloning… 42%") in its card and sidebar row. A failed clone shows the reason (no access, network, disk) and **Retry**. **Use existing folder…** stays as a secondary menu item for users who already have a checkout.

### Project home

Shown when a project has two or more repos. A standalone repo, or a one-repo project, keeps today's home screen.

**Header**: project title (rename inline), repo count, home repo marker. Breadcrumb `Project › repo` once drilled into a repo.

**Code tab**
- Two-column grid of repo cards (one column under ~720px width). Card: repo name, home-repo badge, current branch, uncommitted/conflict counts, workspace count, last commit time, "unavailable" state for repos missing on this machine.
- Order follows `project_repos.position`; home repo first.
- Clicking a card opens that repo's Code tab content as today (file browser and workspace list), scoped to the repo, with a back control to the grid.

**Changes tab**
- One consolidated view of the home checkouts' uncommitted changes across all project repos (not workspace changes), grouped by repo: a collapsible section per repo with its file list and diff, using the existing `changes` screen components.
- Repos with no changes collapse into one "No changes: a, b" line.
- Stage, discard and commit act per repo section; "Commit all" prompts for one message and commits each repo separately (per-repo result, not atomic; see Cross-repo semantics).
- Review comments and viewed-file state stay per repo (existing `pending_reviews` in each repo DB).

**Commits tab**
- One column per repo, side by side (horizontal scroll past ~4 columns; home repo first). Each column card shows that repo's linear commit history (first-parent of the default branch, newest at top).
- Rows are time-aligned across columns: the vertical axis is time, grouped into bands (e.g. Today, Yesterday, then by day, then by week), so commits made at the same time line up across repos and empty bands show gaps.
- A commit shared by a project workspace branch is marked with the workspace name so related commits across repos are easy to spot.
- Clicking a column card opens that repo's full-width Commits list (today's `commits` screen), scoped to the repo, with a back control.

**D16. Time alignment in the Commits tab**
- A. Shared time bands (day, then week), commits stacked inside each band per column; band height = tallest column. **(Recommended)**
- B. Continuous time axis with exact vertical positions.
- C. No alignment; independent lists.

A keeps rows readable when one repo has 30 commits a day and another has 1, and still lines up related work. B leaves large gaps and overlaps. C loses the cross-repo view the tab exists for.

**D17. Changes tab scope**
- A. Home checkouts of all project repos, grouped by repo. **(Recommended; matches the request as read)**
- B. Home checkouts plus every workspace.
- C. Active project workspace only.

Project workspace changes stay on the project workspace screen; B would mix unrelated parallel work into one list.

**Data**: each tab reads per-repo data through the existing `*_data.rs` loaders, one per repo, in parallel, rendering each repo as it arrives. A slow or missing repo shows a placeholder in its card or column, never blocks the others.

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

## Managed clones

Treq, not the user, gets every project repo onto the machine.

- **Location**: `~/.treq/repos/<host>/<owner>/<name>` (configurable root in app settings). Path recorded in `repo_locations`. A repo the user already opened from elsewhere is reused, never cloned twice (match on `repo_key`).
- **When**: on `add-repo <url>`; on first use of a project repo with no local path (open, workspace create, `treq agent -r`, `treq project spawn`); optionally eagerly for all project repos when a project syncs to a new machine (setting, default off for repos over 1 GB).
- **How**: in Rust via `gix` (AGENTS.md: no shelling out to `git`). Partial clone (`blob:none`) by default, then Treq's normal colocated jj setup, so the clone is a regular Treq repo with `.treq/` and workspaces.
- **Auth**: the GitHub token Treq already holds (`github.rs`), then the user's git credential helper / SSH agent. No new credential store.
- **Concurrency**: one clone per `repo_key` at a time (lock file under the managed root); concurrent callers wait on it and then continue. Agents block on the clone with progress output, up to a timeout, then fail with a clear message.
- **Linking**: after clone, Treq adds the repo to the project, creates any requested workspace (shared branch name if it is part of a project workspace), links it into the project root (D3), and adds it to every running project agent's read scope for its next launch. Running agents learn about it from `treq project st`.
- **Removal**: removing a repo from all projects does not delete a managed clone automatically; Settings lists managed clones with size and **Delete clone** (blocked while it has workspaces with unpushed work).

**D18. Who triggers clones**
- A. Treq clones automatically on first use; agents and the CLI only name repos. **(Recommended)**
- B. Explicit `treq repo clone` step required before use.
- C. Eager clone of every project repo on sync.

A matches "the user should not need to clone": any command that names a repo just works. B adds a step agents forget. C wastes disk for repos the user never touches on that machine (still available as a setting).

**D19. Where managed clones live**
- A. One managed root `~/.treq/repos/<host>/<owner>/<name>`, shared by all projects. **(Recommended)**
- B. Inside the project root, per project.
- C. Next to the home repo.

A gives one checkout per repo per machine (D7a) and stable paths for agents. B duplicates repos shared by projects; C depends on where the home repo happens to be.

### CLI for agents

```
treq project add-repo [<project>] <url|owner/name|path> [--no-clone]   # clones if needed, adds to project
treq repo ensure <repo>          # clone if missing; print path; idempotent
treq add -r <repo> <branch>      # clones on demand, then creates the workspace
treq agent -r <repo> <branch> "<prompt>"  # clones and creates the workspace on demand
treq project spawn --plan plan.json       # clones any missing repos first, in parallel
```

`<project>` defaults to the project of the current directory. Output reports `cloned`, `already present` or the failure reason, and `--json` for agents. The orchestrator's system prompt says repos are cloned on demand, so it never runs `git clone` itself.

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

A matches v1 (the agent starts in a real repo, with its own `AGENTS.md`/skills) and gives `treq` a repo to resolve against. B has no git state to commit plans or notes into. C makes "which project context applies" ambiguous for repos in several projects. If the home repo is not on this machine, Treq clones it before starting the orchestrator.

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
- Repo-relative ref to a repo not on this machine: Treq clones it before launch; if the clone fails, skipped with the same warning.
- Context path that contains secrets (`.env`, keys): refuse to add paths matching a deny list; warn on dirs that contain them.
- Huge prompts: total injected text capped (64 KiB); launch warns and truncates lowest-priority prompts last-first.
- Same repo in two projects: an agent uses the project its session was opened from (`sessions.project_id`, else the workspace's `project_id`, else the project active in the sidebar). Never merges two projects' prompts.
- Non-Claude agents without `--add-dir`: paths are listed in the prompt only; the preview says read access is not enforced for that agent.

## Edge cases

- **Same repo in multiple projects**: the normal case (many-to-many through `project_repos`). One checkout per machine, shared by all its projects; repo state stays in the repo's `.treq`. Each project lists all of the repo's workspaces, tagged with the project they were created from; a filter shows "this project only". A workspace belongs to at most one project workspace. Removing a repo from one project does not touch others; a repo removed from its last project stays a standalone repo. Removing the second-to-last repo leaves a one-repo project, which stays (it keeps its prompts and context) until the user deletes it; deleting a project never touches its repos.
- **Missing access** (clone fails: no GitHub permission, auth expired, private fork): repo shown with the failure reason and **Retry**; project workspace ops skip it with a visible warning; the Summary tab omits PRs we cannot read.
- **Renamed repo**: match on `github_node_id`, update `repo_key`; local detection via remote URL change on fetch.
- **Deleted repo / missing path**: mark unavailable, keep row; user removes explicitly. `jj.rs` path-validating functions already return defaults on missing paths.
- **Monorepo**: open standalone, as today. A one-repo project is allowed when the user wants project prompts/context for it. Optional `sparse_patterns` per project repo (workspaces already support them) to scope a project to subdirs; no virtual "sub-repos".
- **Local-only repo (no remote)**: syncs as `local:<uuid>`, shown "available on <machine> only".
- **Sync conflicts**: version mismatch → refetch, field-level merge for repo list (union adds, honour tombstones), last-write-wins for name/config with a toast.
- **Repo path moved or deleted on disk**: a managed clone is re-cloned on next use; a user-owned checkout falls back to a managed clone, keeping workspaces only if the old path reappears. **Use existing folder…** re-points `repo_locations`.
- **Disk full during clone**: clone is removed, error names the size needed; existing clones untouched.
- **Large repos / monorepos**: partial clone plus optional sparse patterns per project repo; eager clone off by default above 1 GB.

## Phased rollout

1. **Model + sync**: Supabase tables/RLS/RPCs, local cache, migration of supporting repos only (D8), sidebar project › repo › workspace tree. Behind `projects` flag.
2. **Managed clones** (D18–D19): `treq repo ensure`, on-demand clone in `add -r`/`agent -r`, clone progress in UI.
3. **Supersede supporting repos**: migrate `supporting_repos` to projects, resolve `-r` against project repos, `treq project st`.
4. **Project prompts and context** (D9–D12): injection in `system_prompt()`, settings section, CLI.
5. **Project workspaces**: shared-branch creation, project root symlinks, project-scoped sessions/agents, summary tab, CLI.
6. **Grouped PRs + push all**, rolled-up checks.
7. **Orchestration**: `treq project spawn`, `parent_session_id`, child list UI, concurrency cap, optional push notifications (D15 B).
8. **Polish**: merge-order hint, sparse monorepo scoping, remove flag.

## Test strategy (TDD)

Each step starts with a failing test.

- `core/projects/` unit tests: repo_key normalisation (ssh/https/.git suffix), override resolution order, branch-name collision detection, status roll-up, PR grouping by project workspace, sync merge rules. Proptests for normalisation and merge.
- Prompt assembly: `system_prompt()` with project prompts in D11 order, repo scoping, disabled prompts skipped, missing context path skipped with warning, size cap, Claude settings/`--add-dir` include context dirs read-only, Cursor prompt file includes them; extend the existing tests in `agent_command.rs`.
- Standalone: opening a repo with no project shows today's UI; **Add Repository…** on a standalone repo creates a named project with it as home; removing from the last project leaves a standalone repo. Plain repos create no project on upgrade.
- Managed clones: `ensure` is idempotent; two concurrent `ensure` calls clone once (lock); existing checkout with same `repo_key` reused; `add -r`/`agent -r`/`spawn` clone missing repos then proceed; auth failure surfaces reason and leaves no partial dir; uses `gix`, never a `git` subprocess (test against a local bare repo fixture, no network).
- Supporting-repos migration: fixture DB with `supporting_repos` → one project with the same repos; `-r` resolves the same names before and after (extend `cli/tests.rs` `-r` cases).
- Orchestration: `spawn` plan parsing and per-entry partial failure; concurrency cap; sub-agent spawn refused (D14); `project st --json` shape across 2 temp repos with stacked workspaces; filesystem settings use project repos.
- `local_db` migration tests with upgrade fixtures for new columns; `db.rs` cache tables.
- Integration (`src-tauri/tests/`): create project workspace across 2 temp jj repos; partial failure leaves successful repos intact; missing path returns defaults.
- CLI tests in `cli/tests.rs` for each subcommand, `--json` shape snapshot.
- GPUI `tests.rs` for project home: 2-col grid (1 col narrow), card click opens repo-scoped Code and back; Changes grouped by repo with home-checkout changes only, "No changes" collapse, per-repo commit; Commits columns per repo, band alignment for skewed commit rates (D16), column click opens full-width repo Commits; one repo slow/missing does not block others; one-repo project shows today's home.
- GPUI `tests.rs`: sidebar switcher, new-project modal, unavailable-repo row, summary tab; QA with `app-qa`.
- Supabase: RLS tests (other user cannot read/write; anon denied), `version_conflict`, tombstones, via `service-qa` against the local stack; `npm test` for any Edge Function.
- Bench: status roll-up for 10 repos × 20 workspaces under `src-tauri/benches/`.

## Open questions

- Should projects also rename repo-scoped settings keys in `db.rs`, or keep repo keys and add project keys?
- Project root symlinks on Windows (junctions vs. copy-free alternative)?
- Do mobile/remote (`mobile/`, `remote/`) surfaces need projects in Phase 1?

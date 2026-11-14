---
sidebar_position: 4
description: Put git worktrees next to the repository, not inside it. A nested worktree shows up as untracked and git add -A stages it as an embedded repository.
---

# Should Git Worktrees Go Inside the Repo or Next to It?

_Next to it. A worktree nested inside the main checkout needs an ignore rule, or Git and your tools treat it as part of the project._

## The Fix

Create worktrees in a sibling directory:

```bash
git worktree add -b feature ../repo-feature
```

If you want them inside the repository, ignore the folder first. `.git/info/exclude` keeps the rule out of the shared `.gitignore`:

```bash
echo "/worktrees/" >> .git/info/exclude
git worktree add -b feature worktrees/feature
```

## Why It Happens

A linked worktree is a full checkout with a `.git` file at its root. Inside the main checkout, Git sees an untracked directory:

```text
$ git worktree add -b nested ./wt-nested
$ git status --short
?? wt-nested/
```

`git add -A` then stages it as a gitlink, the same entry a submodule uses. Git 2.43 warns:

```text
warning: adding embedded git repository: wt-nested
hint: You've added another git repository inside your current repository.
```

If you commit that, the repository records a submodule entry with no `.gitmodules` file and no URL.

Git ignores the nested folder once an ignore rule covers it, but other tools may not. `grep -r`, test runners, and file watchers walk the directory tree. In a quick check, `grep -rl` from the main checkout matched both `./a.txt` and `./wt-nested/a.txt`. A test runner can find every test twice, and a dev server can rebuild when another worktree changes.

## Choosing a Layout

| Layout | Example | Needs an ignore rule |
| --- | --- | --- |
| Sibling | `../repo-feature` | No |
| Dedicated folder beside the repo | `../repo.worktrees/feature` | No |
| Nested | `./worktrees/feature` | Yes, plus tool config |

A dedicated folder beside the repository keeps many worktrees together without nesting them. `git worktree list` shows every path either way.

## How Treq Handles It

Treq nests [workspaces](/docs/concepts/workspaces) under `.treq/workspaces/`. It writes `.treq/.gitignore` so Git ignores everything there except `config.yaml` and `workflows/`.

## Related

- [Worktree or clone to review a pull request?](./worktree-or-clone-to-review-a-pr)
- [Remove a locked git worktree](./remove-locked-worktree)
- [What are Git Worktrees?](/learn/concepts/git/git-worktrees)

---
sidebar_position: 11
description: AI-generated branches go stale quickly. Here is why auto-rebase matters and how to use it safely in review workflows.
---

# Auto-Rebasing AI Workspaces

_Keep agent branches current when their base moves, without losing review control._

## Goal

Understand branch drift in AI workflows, rebase dependent work safely, and use Treq auto-rebase without treating a successful rebase as an approval.

## What branch drift is

**Branch drift** happens when `main` or a stack parent moves while an agent branch still points at an older commit. The branch still builds against yesterday's code. Merging it later surfaces conflicts and behavioral surprises that would have been cheaper to fix earlier.

AI branches drift faster than typical human branches because agents finish drafts quickly, then wait in a review queue while other merges continue.

## Stack drift examples

```text
Day 1
main @ A
└── auth/schema @ B
    └── auth/api @ C     # based on B

Day 2 after schema review edits
main @ A
└── auth/schema @ B'     # rewritten during review
    └── auth/api @ C     # still based on B  ← drifted
```

Independent parallel branches drift against `main` the same way:

```bash
# agent branch created Monday
git log --oneline origin/main..HEAD

# Wednesday, after several merges to main
git fetch origin
git merge-base --is-ancestor origin/main HEAD || echo "branch has drifted"
```

## Safe rebasing

Rebase only branches whose rewrite policy you control. Use a lease when force-pushing:

```bash
git fetch origin
git rebase origin/main
# resolve conflicts, run tests
git push --force-with-lease
```

For a stack, restack from the changed layer upward:

```bash
git switch auth/api
git rebase auth/schema
git push --force-with-lease origin auth/api
```

After every rebase:

1. Read the new diff against the intended base.
2. Re-run the relevant tests.
3. Treat conflict resolutions as new code under review. [Git rerere](/learn/concepts/git/git-rerere) can reuse a prior resolution across rebases, and that reused result still needs the same review.

```bash
git diff origin/main...HEAD
npm test
```

Do not rebase a branch other people are building on unless they expect force pushes.

## Review implications

Auto-rebase changes commits under an open review. Reviewers must know when a diff moved. A green rebase means Git applied patches. It does not mean the conflict resolutions are correct.

Pause upper-layer review while the lower layer is still being rewritten. Restacking onto an unstable base creates repeated review churn.

## Treq implementation

Treq records each workspace's `target_branch`. When that target moves, Treq rebases dependent workspaces onto the new commit in stack order. It skips a workspace that already descends from the target and records the target commit used by a successful rebase.

Conflicts stop the clean flow. Resolve the lowest conflict first, then let dependents update. You can send conflict resolution instructions to an agent from the review UI. See [Workspaces](/docs/concepts/workspaces) and [Changes and Reviews](/docs/concepts/changes-and-reviews).

Pending working-copy edits can block or complicate synchronization. Commit, move, or discard them before relying on an automatic rebase result.

## Common failure cases

**Rebase succeeds, behavior breaks.** A conflict marker resolution kept the wrong side. Re-read conflicted files and add a regression test. [zdiff3](/learn/concepts/git/zdiff3) makes those markers easier to read when both sides share surrounding lines.

**Force push without lease.** Concurrent updates on the remote branch can be overwritten, so always push with [`--force-with-lease`](/learn/concepts/git/force-with-lease).

**Rebasing during an active agent session.** The agent may write files while history rewrites. Stop the agent or wait until it finishes before rebasing that workspace.

**Deep stacks with a noisy root.** Frequent root rewrites punish every dependent layer. Stabilize the root before adding height.

## Related

- [What Are Stacked PRs in AI-Assisted Development](/learn/stacked-prs)
- [How to Fix Merge Conflicts Created by Coding Agents](/learn/how-to/merge-conflicts-with-coding-agents)
- [Merge vs Rebase](/learn/concepts/git/merge-vs-rebase)
- [What is Git Rerere?](/learn/concepts/git/git-rerere)
- [Rebasing and Stacked Branch Answers](/learn/answers/rebasing-and-stacks)
- [Workspaces](/docs/concepts/workspaces)

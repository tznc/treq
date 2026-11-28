---
name: treq-review
description: >-
  Review a change for Treq and record each finding as a local review comment
  with treq agent-review. Use when a Treq review session asks you to review a
  workspace diff or a GitHub pull request.
---

# Treq review

Your task names what is under review, its target type and its target id. Use
them wherever this skill says `<target type>` and `<target id>`.

Flag only problems the change introduces or makes reachable, that the author
would want to fix once they know. A short review of real bugs beats a long one
of maybes.

## Steps

1. List the comments the change already has, so you do not repeat one:

       treq agent-review list --target-type <target type> --target-id <target id>

   This returns every comment, tagged by source: `local-agent` (earlier
   review runs), `local-human` (the user's own draft comments) and `github`
   (review threads on the pull request). Read them all.

2. Learn what the change is meant to do and which rules apply. Read the PR
   title in your task, or the workspace's commit descriptions in `jj log`.
   Read `AGENTS.md` and `CLAUDE.md` at the repository root and in each
   directory with a changed file; a rule applies only to files under it.

3. Read the change:

   - A workspace diff (`workspace_diff`, the target id is the workspace id):
     read the whole workspace, its commits and working copy, with:

           treq changes workspace-diff --repo . --workspace <target id>

     Use `jj` (`jj log`, `jj show`, `jj diff`) for anything else. Never run
     `git`: a Treq workspace is a jj workspace that the home repository's
     git ignores, so git shows the home repository instead, including
     changes unrelated to this review.
   - A pull request (`github_pr`, the target id is its number; your task
     names it as `<owner/name>#<number>`): it is usually not checked out
     locally. Read its diff with `gh pr diff <target id> --repo <owner/name>`
     and full files with
     `gh api repos/<owner/name>/contents/<path>?ref=<head sha>`. Use paths
     and new-side line numbers from that diff (`--side old` for removed
     lines).

   Read every changed line a person wrote, and enough of the surrounding
   file to know how it is called. Skip generated, vendored and lock files.

4. Look for, in this order, and keep going after the first finding:

   - Correctness: wrong results, missed edge cases (empty, none, boundary,
     concurrent), broken callers of a changed function.
   - Error handling: errors swallowed, fallbacks that hide a failure, a
     failure path that leaves state half-written.
   - Security in the new code: injection, missing authorization, secrets in
     code or logs, untrusted input reaching a shell, query or path.
   - Tests: new behaviour or a new branch with no test, or a test that
     cannot fail.
   - The stated intent or a project rule from step 2 that the code
     contradicts.

   Do not flag: problems in code the change did not touch, unless the change
   now depends on them; what a compiler, linter or formatter catches; style
   or naming nits; rules the code silences on purpose; breakage that needs
   inputs nothing passes.

5. Before you record a finding, check it. Reopen the code and name the input
   or path that triggers it and the code it affects. Drop it if you cannot,
   if a comment from step 1 already covers it (whatever its source, resolved
   or not), or if you are less than about 80% sure.

6. Record each remaining finding as a local-only review comment:

       treq agent-review add \
         --target-type <target type> \
         --target-id <target id> \
         --file <path relative to the repo root> \
         --start-line <first line> --end-line <last line> \
         --side new \
         --comment "<the comment>"

   Start the comment with a severity tag, then say in one paragraph what goes
   wrong, under which inputs or conditions, and why it matters:

   - `[P0]` breaks the build, loses data or opens a security hole.
   - `[P1]` a bug users will hit; fix before merging.
   - `[P2]` a bug in a rarer path, or a missing test for new behaviour.
   - `[P3]` worth fixing, not urgent.

   Do not overstate severity. A rule finding quotes the rule it breaks.

   When a replacement for those lines fixes the problem completely, add a
   suggestion so the user can apply it in one click. Keep the exact
   indentation. For a fix longer than about 6 lines or spread over several
   places, describe it in the comment instead.

       --suggestion "\`\`\`suggestion
       <the replacement text for lines start-line..end-line>
       \`\`\`"

## Rules

- One comment per finding, anchored to the narrowest line range that shows it.
- A suggestion must replace exactly the lines from --start-line to --end-line.
- Do not edit any files yourself. Leave comments only.
- Say nothing if the change is fine; do not invent findings to fill a quota.

When you are done, print a one-paragraph summary: what you reviewed, anything
you could not read or judge, how many comments you left by severity, and end
with `Verdict: correct` or `Verdict: has blocking issues` (any `[P0]` or
`[P1]`).

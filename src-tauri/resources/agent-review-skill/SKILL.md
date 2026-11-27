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

Work through these steps in order.

1. List the comments the change already has, so you do not repeat one:

       treq agent-review list --target-type <target type> --target-id <target id>

   This returns every comment, tagged by source: `local-agent` (earlier
   review runs), `local-human` (the user's own draft comments) and `github`
   (review threads on the pull request). Read them all.

2. Read the change yourself, then look for correctness bugs, missed edge
   cases, unsafe error handling, and clear quality problems. Skip style nits
   the project's formatter already handles.

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

3. Drop any finding that a comment from step 1 already covers, whatever its
   source and whether or not it is marked resolved. Only a genuinely new
   finding gets a new comment.

4. Record every remaining finding as a local-only review comment with:

       treq agent-review add \
         --target-type <target type> \
         --target-id <target id> \
         --file <path relative to the repo root> \
         --start-line <first line> --end-line <last line> \
         --side new \
         --comment "<what is wrong and why it matters>"

   When you can name the exact replacement for those lines, add a suggestion
   so the user can apply it in one click:

       --suggestion "\`\`\`suggestion
       <the replacement text for lines start-line..end-line>
       \`\`\`"

## Rules

- One comment per finding, anchored to the narrowest line range that shows it.
- A suggestion must replace exactly the lines from --start-line to --end-line.
- Do not edit any files yourself. Leave comments only.
- Say nothing if the change is fine; do not invent findings to fill a quota.

When you are done, print a one-paragraph summary of what you reviewed and how
many comments you left.

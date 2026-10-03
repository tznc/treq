import { AGENT_REVIEW_TARGET_WORKSPACE_DIFF } from "./api-types-review";

export interface ReviewPromptVars {
  /** Target type discriminator, e.g. "workspace_diff". */
  targetType: string;
  /** Identifier within the target type, e.g. a workspace id. */
  targetId: string;
  /** Short human summary of what is being reviewed. */
  diffSummary: string;
}

/**
 * Built-in review prompt. A repo can override it with a custom prompt in
 * repository settings; both go through {@link renderReviewPrompt}, so a custom
 * prompt can use the same `{target_type}` / `{target_id}` / `{diff_summary}`
 * placeholders.
 */
export const DEFAULT_REVIEW_PROMPT = `You are reviewing the current changes in this repository.

What is under review: {diff_summary}

Work through these steps in order.

1. List the comments this diff already has, so you do not repeat one:

     treq agent-review list --target-type {target_type} --target-id {target_id}

   This returns every comment on the diff, tagged by source: \`local-agent\`
   (earlier review runs), \`local-human\` (the user's own draft comments) and
   \`github\` (review threads on the pull request). Read them all.

2. Read the diff yourself (for example with \`treq diff\` or \`git diff\`), then
   look for correctness bugs, missed edge cases, unsafe error handling, and
   clear quality problems. Skip style nits the project's formatter already
   handles.

3. Drop any finding that a comment from step 1 already covers, whatever its
   source and whether or not it is marked resolved. Only a genuinely new
   finding gets a new comment.

4. Record every remaining finding as a local-only review comment with:

  treq agent-review add \\
    --target-type {target_type} \\
    --target-id {target_id} \\
    --file <path relative to the repo root> \\
    --start-line <first line> --end-line <last line> \\
    --side new \\
    --comment "<what is wrong and why it matters>"

When you can name the exact replacement for those lines, add a suggestion so the
user can apply it in one click:

    --suggestion "\\\`\\\`\\\`suggestion
<the replacement text for lines start-line..end-line>
\\\`\\\`\\\`"

Rules:
- One comment per finding, anchored to the narrowest line range that shows it.
- A suggestion must replace exactly the lines from --start-line to --end-line.
- Do not edit any files yourself. Leave comments only.
- Say nothing if the change is fine; do not invent findings to fill a quota.

When you are done, print a one-paragraph summary of what you reviewed and how
many comments you left.`;

/** Fill `{target_type}` / `{target_id}` / `{diff_summary}` placeholders. */
export function renderReviewPrompt(
  template: string,
  vars: ReviewPromptVars,
): string {
  return template
    .split("{target_type}")
    .join(vars.targetType)
    .split("{target_id}")
    .join(vars.targetId)
    .split("{workspace_id}")
    .join(vars.targetId)
    .split("{diff_summary}")
    .join(vars.diffSummary);
}

/** Convenience wrapper: custom prompt when set, otherwise the built-in one. */
export function buildReviewPrompt(
  customPrompt: string | null | undefined,
  vars: ReviewPromptVars,
): string {
  const template = customPrompt?.trim() ? customPrompt : DEFAULT_REVIEW_PROMPT;
  return renderReviewPrompt(template, vars);
}

/** Summary line used when a workspace diff is what is being reviewed. */
export function workspaceDiffSummary(
  branchName: string | undefined,
  fileCount: number,
): string {
  const files = `${fileCount} changed file${fileCount === 1 ? "" : "s"}`;
  return branchName
    ? `the ${branchName} workspace diff (${files})`
    : `the current workspace diff (${files})`;
}

/**
 * Built-in prompt for reviewing Linear content. Separate from the code review
 * prompt: a repo's custom review prompt is written for diffs, so it does not
 * apply here. Placeholders: `{target_type}`, `{target_id}`, `{diff_summary}`
 * (what is reviewed) and `{snapshot_dir}` (where the app wrote the text).
 */
export const DEFAULT_LINEAR_REVIEW_PROMPT = `You are reviewing Linear content: {diff_summary}.

treq copied the text under review into {snapshot_dir}:
- INDEX.md lists the files below, who wrote each comment, and which passage a
  comment is anchored to.
- body.md is the text of the issue, project or document itself.
- comments/<id>.md is the body of each comment on it.

Work through these steps in order.

1. List the review comments already left, so you do not repeat one:

     treq agent-review list --target-type {target_type} --target-id {target_id}

2. Read INDEX.md, body.md and every comment file. Check claims about the code
   against this repository when they matter.

3. Analyse the text. In body.md, look for unclear or ambiguous requirements,
   missing acceptance criteria, contradictions, claims that do not match the
   code, missed edge cases and risks, and errors that change the meaning. In
   the comments, look for open questions nobody answered, disagreements left
   unresolved, decisions made in a comment that body.md does not reflect yet,
   and statements that are wrong.

4. Drop any finding that a comment from step 1 already covers.

5. Record every remaining finding with:

  treq agent-review add \\
    --target-type {target_type} \\
    --target-id {target_id} \\
    --file <body.md or comments/<id>.md> \\
    --start-line <first line> --end-line <last line> \\
    --comment "<what is wrong and why it matters>"

When you can write the replacement for those lines, add a suggestion so the
user can apply it to Linear in one click:

    --suggestion "\\\`\\\`\\\`suggestion
<the replacement text for lines start-line..end-line>
\\\`\\\`\\\`"

Rules:
- One comment per finding, anchored to the narrowest line range that shows it.
- A suggestion must replace exactly the lines from --start-line to --end-line.
- When a comment settles something the body should say, anchor the finding on
  body.md and suggest the new body text there.
- A suggestion on a comment file can only be applied by that comment's author.
- Do not edit files or change anything in Linear yourself. Leave comments only.
- Say nothing if the text is fine; do not invent findings to fill a quota.

When you are done, print a one-paragraph summary of what you reviewed and how
many comments you left.`;

/** Fills the Linear prompt's placeholders. */
export function buildLinearReviewPrompt(
  vars: ReviewPromptVars & { snapshotDir: string },
): string {
  return renderReviewPrompt(DEFAULT_LINEAR_REVIEW_PROMPT, vars)
    .split("{snapshot_dir}")
    .join(vars.snapshotDir);
}

export { AGENT_REVIEW_TARGET_WORKSPACE_DIFF };

#!/usr/bin/env node
// A stand-in `gh` for the landing capture: answers the calls the app makes
// (PR status, checks, the GitHub panel's lists, review threads) with fixture
// data for lorem-ipsum/event-bus. No network, no token.
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
if (process.env.TREQ_FAKE_GH_LOG) {
  appendFileSync(process.env.TREQ_FAKE_GH_LOG, `${JSON.stringify(args)}\n`);
}
// A QA spec signs gh out by leaving this file in HOME.
if (existsSync(join(process.env.HOME ?? "", "fake-gh-signed-out"))) {
  process.stderr.write("To get started with GitHub CLI, please run:  gh auth login\n");
  process.exit(4);
}
const has = (flag) => args.includes(flag);
const after = (flag) => args[args.indexOf(flag) + 1];
const field = (prefix) =>
  args.find((arg) => arg.startsWith(`${prefix}=`))?.slice(prefix.length + 1);

const REPO = "https://github.com/lorem-ipsum/event-bus";
const author = (login) => ({ login, avatarUrl: null });
/** Pull requests assigned to octocat, none of them octocat's own. */
const ASSIGNED_TO_OCTOCAT = new Set([409, 421]);

const prs = [
  {
    number: 418,
    title: "feat: handle empty event messages",
    headRefName: "feat/empty-event-message",
    baseRefName: "feat/event-ingest",
    mergeStateStatus: "UNSTABLE",
  },
  {
    number: 421,
    title: "fix: retry Slack webhooks on 429",
    headRefName: "fix/slack-retry",
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
    author: author("alice"),
    createdAt: "2026-08-12T09:00:00Z",
  },
  {
    number: 412,
    title: "feat: ingest Discord and Slack events",
    headRefName: "feat/event-ingest",
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
  },
  {
    number: 415,
    title: "feat(spool): fail readiness when the spool queue is unreachable",
    headRefName: "feat/spool-readiness",
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
    author: author("erin"),
    createdAt: "2026-08-08T09:00:00Z",
    body: [
      "## Summary",
      "",
      "`/readyz` now returns **503** while the spool queue can't be reached, so the",
      "load balancer stops routing ingest traffic to a pod that would drop it.",
      "",
      "- Probes the queue with a 500 ms timeout",
      "- Keeps `/healthz` green, so the pod isn't restarted",
    ].join("\n"),
  },
  {
    number: 409,
    title: "docs: add the ingest runbook",
    headRefName: "docs/ingest-runbook",
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
    author: author("bob"),
    createdAt: "2026-07-29T09:00:00Z",
  },
  {
    number: 401,
    title: "feat: keyvalues cache",
    headRefName: "feat/keyvalues-cache",
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
  },
  {
    number: 388,
    title: "feat: alerting logs",
    headRefName: "feat/alerting-logs",
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
    state: "MERGED",
  },
].map((pr) => ({
  state: "OPEN",
  url: `${REPO}/pull/${pr.number}`,
  author: author("octocat"),
  labels: [],
  createdAt: "2026-08-01T10:00:00Z",
  updatedAt: "2026-08-12T10:00:00Z",
  isDraft: false,
  body: "",
  comments: [],
  ...pr,
}));

// QA specs change a PR on "GitHub" by writing `$HOME/fake-gh-prs.json`:
// fields to merge over a fixture PR, by head branch.
const overrides = join(process.env.HOME ?? "", "fake-gh-prs.json");
if (existsSync(overrides)) {
  const changes = JSON.parse(readFileSync(overrides, "utf8"));
  for (const pr of prs) Object.assign(pr, changes[pr.headRefName]);
}
// `gh pr edit` titles and bodies, kept beside the call log so later calls
// see them, as GitHub would.
const prEditsFile = process.env.TREQ_FAKE_GH_LOG
  ? join(dirname(process.env.TREQ_FAKE_GH_LOG), "gh-pr-edits.json")
  : null;
const prEdits =
  prEditsFile && existsSync(prEditsFile)
    ? JSON.parse(readFileSync(prEditsFile, "utf8"))
    : {};
for (const pr of prs) Object.assign(pr, prEdits[pr.number]);

const issues = [
  {
    number: 77,
    title: "Empty event payloads drop the Discord body",
    author: author("alice"),
    createdAt: "2026-08-10T10:00:00Z",
  },
  {
    number: 81,
    title: "Document retry/backoff for ingest",
    author: author("bob"),
    createdAt: "2026-08-11T10:00:00Z",
  },
  {
    number: 74,
    title: "Slack thread replies arrive out of order",
    author: author("carol"),
    createdAt: "2026-08-06T10:00:00Z",
  },
  {
    number: 69,
    title: "Alerting logs miss the provider name",
    author: author("dave"),
    createdAt: "2026-08-02T10:00:00Z",
  },
  {
    number: 66,
    title: "Web feed shows raw JSON for bot messages",
    author: author("erin"),
    createdAt: "2026-07-30T10:00:00Z",
  },
  {
    number: 61,
    title: "Cache keys collide across workspaces",
    author: author("frank"),
    createdAt: "2026-07-27T10:00:00Z",
  },
  {
    number: 58,
    title: "Add a dead-letter queue for failed deliveries",
    author: author("alice"),
    createdAt: "2026-07-24T10:00:00Z",
  },
].map((issue) => ({
  state: "OPEN",
  url: `${REPO}/issues/${issue.number}`,
  updatedAt: issue.createdAt,
  labels: { nodes: [] },
  ...issue,
}));

function check(name, bucket, minutes) {
  const started = new Date(Date.parse("2026-08-12T10:00:00Z"));
  const completed = new Date(started.getTime() + minutes * 60_000);
  return {
    name,
    bucket,
    link: `${REPO}/actions/runs/${name}`,
    startedAt: started.toISOString(),
    completedAt: bucket === "pending" ? null : completed.toISOString(),
  };
}

const checks = {
  "feat/empty-event-message": [
    check("build", "pass", 4),
    check("test", "fail", 2),
    check("lint", "pass", 1),
    check("typecheck", "pending", 1),
  ],
  "feat/event-ingest": [
    check("build", "pass", 3),
    check("test", "pass", 9),
    check("lint", "pass", 1),
    check("typecheck", "pass", 2),
  ],
  "feat/keyvalues-cache": [
    check("build", "pass", 2),
    check("test", "pass", 5),
    check("lint", "pass", 1),
  ],
  "feat/alerting-logs": [check("build", "pass", 3), check("test", "pass", 8)],
};

const reviewThreads = {
  418: [
    {
      id: "PRRT_client_stringify",
      isResolved: true,
      isOutdated: false,
      path: "packages/web/src/lib/client.ts",
      line: 7,
      diffSide: "RIGHT",
      comments: {
        nodes: [
          {
            id: "PRRC_client_stringify",
            body: "JSON.stringify is the right fallback when event_message is empty.",
            createdAt: "2026-08-12T14:22:00Z",
            url: `${REPO}/pull/418#discussion_r1`,
            diffHunk:
              '@@ -0,0 +1,7 @@\n+import type { EventBody } from "../../../api/src/schema";\n+\n+export async function postEvent(body: EventBody): Promise<Response> {\n+  const message =\n+    typeof body.event_message === "string" && body.event_message.length > 0\n+      ? body.event_message\n+      : JSON.stringify(body);',
            author: author("octocat"),
            reactionGroups: [
              { content: "THUMBS_UP", viewerHasReacted: false, reactors: { totalCount: 2 } },
              { content: "ROCKET", viewerHasReacted: true, reactors: { totalCount: 1 } },
            ],
          },
          {
            id: "PRRC_client_stringify_reply",
            body: "Agreed :robot: **one caveat**:\n\n- `JSON.stringify` drops `undefined` fields\n- see [MDN](https://developer.mozilla.org/)",
            createdAt: "2026-08-12T15:02:00Z",
            url: `${REPO}/pull/418#discussion_r2`,
            diffHunk: "",
            author: author("hubot"),
          },
        ],
      },
    },
  ],
};

// Issue #77's conversation, with reactions as `gh issue view` reports them
// and whether octocat reacted, as GraphQL `nodes` reports it.
const issueComments = {
  77: [
    {
      id: "IC_discord_body",
      body: "Reproduced with a Discord embed-only message: the body comes through as an empty string.",
      author: author("bob"),
      createdAt: "2026-08-10T12:00:00Z",
      reactionGroups: [
        { content: "THUMBS_UP", users: { totalCount: 3 } },
        { content: "HEART", users: { totalCount: 1 } },
        { content: "EYES", users: { totalCount: 2 } },
      ],
    },
    {
      id: "IC_discord_fix",
      body: "Falling back to the serialized payload should cover it.",
      author: author("alice"),
      createdAt: "2026-08-10T15:30:00Z",
      reactionGroups: [],
    },
  ],
};
const viewerReacted = { IC_discord_body: ["THUMBS_UP"] };

function pick(item, fields) {
  return Object.fromEntries(fields.split(",").map((key) => [key, item[key]]));
}

function out(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
  process.exit(0);
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

const [group, command] = args;
const branch = args[args.indexOf("--") + 1];

if (group === "--version") out("gh version 2.60.0 (fake)");
if (group === "auth") {
  process.stdout.write(
    "github.com\n  ✓ Logged in to github.com account octocat\n",
  );
  process.exit(0);
}
if (group === "pr" && command === "list") {
  const state = after("--state");
  let list = prs.filter((pr) =>
    state === "all"
      ? true
      : state === "merged"
        ? pr.state === "MERGED"
        : pr.state === state.toUpperCase(),
  );
  if (has("--draft")) list = list.filter((pr) => pr.isDraft);
  // The signed-in user is octocat.
  if (after("--author") === "@me") list = list.filter((pr) => pr.author.login === "octocat");
  if ((after("--search") ?? "").split(/\s+/).includes("assignee:@me"))
    list = list.filter((pr) => ASSIGNED_TO_OCTOCAT.has(pr.number));
  let words = (has("--search") ? after("--search") : "")
    .split(/\s+/)
    .filter((word) => word && !word.includes(":"))
    .map((word) => word.toLowerCase());
  // QA hooks: "slow" holds the answer back so the search's loading state
  // can be captured, "unreachable" fails as the network would.
  if (words.includes("unreachable"))
    fail("HTTP 502: Bad Gateway (https://api.github.com/graphql)");
  if (words.includes("slow")) {
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 4000);
    words = words.filter((word) => word !== "slow");
  }
  list = list.filter((pr) => words.every((word) => pr.title.toLowerCase().includes(word)));
  out(list.map((pr) => pick(pr, after("--json"))));
}
if (group === "pr" && command === "edit") {
  const number = args[2];
  const edit = { ...prEdits[number] };
  if (has("--title")) edit.title = after("--title");
  if (has("--body-file")) edit.body = readFileSync(0, "utf8");
  if (prEditsFile) {
    writeFileSync(prEditsFile, JSON.stringify({ ...prEdits, [number]: edit }));
  }
  process.exit(0);
}
if (group === "pr" && command === "view") {
  const key = has("--") ? branch : args[2];
  const pr = prs.find((p) => p.headRefName === key || String(p.number) === key);
  if (!pr) fail(`no pull requests found for branch "${key}"`);
  out(pick(pr, after("--json")));
}
if (group === "pr" && command === "checks") {
  const key = has("--")
    ? branch
    : prs.find((p) => String(p.number) === args[2])?.headRefName;
  const list = checks[key];
  if (!list) fail(`no pull requests found for branch "${key}"`);
  out(list);
}
if (group === "pr" && command === "diff") {
  // A test can give a PR a diff, and change it mid-run, by writing
  // pr-diff-<n>.diff next to this script; read on every call.
  const file = join(dirname(fileURLToPath(import.meta.url)), `pr-diff-${args[2]}.diff`);
  if (existsSync(file)) process.stdout.write(readFileSync(file, "utf8"));
  process.exit(0);
}
if (group === "api" && /^repos\/[^/]+\/[^/]+\/pulls\/\d+\/reviews$/.test(command ?? "")) {
  out({ id: 1, state: "COMMENTED" });
}
if (group === "api" && command === "graphql") {
  const input =
    after("--input") === "-" ? JSON.parse(readFileSync(0, "utf8")) : null;
  const query = input?.query ?? field("query") ?? "";
  if (query.includes("nodes(ids")) {
    const comments = Object.values(issueComments).flat();
    out({
      data: {
        nodes: input.variables.ids.map((id) => {
          const comment = comments.find((c) => c.id === id);
          if (!comment) return null;
          return {
            id,
            reactionGroups: comment.reactionGroups.map(({ content, users }) => ({
              content,
              viewerHasReacted: (viewerReacted[id] ?? []).includes(content),
              reactors: users,
            })),
          };
        }),
      },
    });
  }
  if (query.includes("addReaction") || query.includes("removeReaction")) {
    out({ data: {} });
  }
  if (query.includes("reviewThreads")) {
    out({
      data: {
        repository: {
          pullRequest: {
            reviewThreads: {
              pageInfo: { hasNextPage: false, endCursor: null },
              nodes: reviewThreads[field("number")] ?? [],
            },
          },
        },
      },
    });
  }
  if (query.includes("issues(")) {
    out({
      data: {
        repository: {
          issues: {
            nodes: query.includes("CLOSED") ? [] : issues,
            pageInfo: { hasNextPage: false, endCursor: null },
          },
        },
      },
    });
  }
  out({ data: {} });
}
if (group === "api" && command === "user") {
  if (after("--jq") === ".login") {
    process.stdout.write("octocat\n");
    process.exit(0);
  }
  out({ login: "octocat" });
}
// The merge preview's GitHub merge: squash by default, rebase merges off.
if (group === "repo" && command === "view") {
  out({
    viewerDefaultMergeMethod: "SQUASH",
    mergeCommitAllowed: true,
    squashMergeAllowed: true,
    rebaseMergeAllowed: false,
  });
}
if (group === "issue" && command === "view") {
  const issue = issues.find((i) => String(i.number) === args[2]);
  if (!issue) fail("issue not found");
  out({ ...issue, body: "", labels: [], comments: issueComments[issue.number] ?? [] });
}
fail(`fake gh: unsupported call: gh ${args.join(" ")}`);

// A fake Linear GraphQL endpoint for the QA app (`TREQ_E2E_LINEAR_URL`):
// the viewer, teams, a short issue list, projects with documents, and
// comments. Only a repository with a Linear API key reaches it, so views
// without one still show "not connected"; the key `lin_api_bad` is rejected
// the way Linear rejects a revoked key.
import { createServer, type Server } from "node:http";

const viewer = { id: "u-octocat", name: "octocat" };
const teammate = { id: "u-hubot", name: "hubot" };

const teams = [
  { id: "t-eng", name: "Engineering", key: "ENG" },
  { id: "t-ops", name: "Operations", key: "OPS" },
];

function issue(
  n: number,
  title: string,
  state: [string, string],
  assignee: typeof viewer | null,
  priority: [number, string],
  project: string | null,
  labels: string[] = [],
  parent: number | null = null,
) {
  return {
    id: `issue-${n}`,
    identifier: `ENG-${n}`,
    title,
    description: `${title}.`,
    state: { name: state[0], type: state[1] },
    labels: { nodes: labels.map((name) => ({ name })) },
    branchName: `octocat/eng-${n}-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`,
    parent: parent === null ? null : { id: `issue-${parent}`, identifier: `ENG-${parent}` },
    children: { nodes: [] as { id: string }[] },
    url: `https://linear.app/lorem/issue/ENG-${n}`,
    assignee,
    priority: priority[0],
    priorityLabel: priority[1],
    project: project && { id: `p-${project}`, name: project },
  };
}

const issues = [
  issue(142, "Retry webhook deliveries", ["In Progress", "started"], viewer, [2, "High"], "Ingest", ["Bug"]),
  issue(139, "Cache key-value lookups", ["Todo", "unstarted"], viewer, [3, "Medium"], null, ["Feature"]),
  issue(131, "Document the ingest runbook", ["Backlog", "backlog"], viewer, [0, "No priority"], "Docs"),
  issue(128, "Drop the v1 events table", ["Done", "completed"], viewer, [4, "Low"], null),
  issue(125, "Rate-limit the public API", ["Todo", "unstarted"], teammate, [1, "Urgent"], "API", ["Feature"]),
  issue(143, "Back off on 429 responses", ["Todo", "unstarted"], viewer, [2, "High"], "Ingest", ["Bug"], 142),
];
issues[0].children.nodes.push({ id: "issue-143" });

const projects = [
  {
    id: "p-Ingest",
    name: "Ingest",
    description: "Reliable webhook ingestion.",
    state: "started",
    targetDate: "2026-11-30",
    progress: 0.4,
    url: "https://linear.app/lorem/project/ingest",
    lead: viewer,
  },
  {
    id: "p-Docs",
    name: "Docs",
    description: "Runbooks and guides.",
    state: "planned",
    targetDate: null,
    progress: 0.1,
    url: "https://linear.app/lorem/project/docs",
    lead: teammate,
  },
  {
    id: "p-API",
    name: "API",
    description: "Public API hardening.",
    state: "backlog",
    targetDate: null,
    progress: 0,
    url: "https://linear.app/lorem/project/api",
    lead: teammate,
  },
];

const documents: Record<string, object[]> = {
  "p-Ingest": [
    {
      id: "d-retries",
      title: "Retry policy",
      content: "# Retry policy\n\nRetry failed deliveries with exponential backoff, at most **5** times.",
      url: "https://linear.app/lorem/document/retry-policy",
      updatedAt: "2026-10-20T10:00:00.000Z",
    },
  ],
};

const comments = [
  {
    id: "c-1",
    body: "Seeing this on the staging cluster too.",
    user: teammate,
    createdAt: "2026-10-21T09:30:00.000Z",
    quotedText: null,
  },
  {
    id: "c-2",
    body: "I'll take it, starting with the backoff.",
    user: viewer,
    createdAt: "2026-10-21T11:05:00.000Z",
    quotedText: null,
  },
];

/** Comments by what they are on. */
const commentsOn: Record<string, object[]> = {
  "issue-142": comments,
  "d-retries": [
    {
      id: "c-3",
      body: "Cap the backoff at ten minutes.",
      user: teammate,
      createdAt: "2026-10-21T14:20:00.000Z",
      quotedText: "exponential backoff",
    },
  ],
};

function answer(query: string, variables: Record<string, unknown>): object | null {
  const id = variables.id as string | undefined;
  // Most specific first: `issue(id:` with comments before `issue(`, and
  // `issues(` / `projects(` before their singular forms.
  if (query.includes("comments(")) {
    const entity = query.match(/\b(issue|project|document)\(id/)?.[1] ?? "issue";
    return { [entity]: { comments: { nodes: commentsOn[id ?? ""] ?? [] } } };
  }
  if (query.includes("documents(")) return { project: { documents: { nodes: documents[id ?? ""] ?? [] } } };
  if (query.includes("viewer")) return { viewer };
  if (query.includes("teams(")) return { teams: { nodes: teams } };
  if (query.includes("issues(")) {
    const filter = variables.filter as { team?: { key?: { eq?: string } } } | null;
    const team = filter?.team?.key?.eq;
    const nodes = team && team !== "ENG" ? [] : issues;
    return { issues: { nodes, pageInfo: { hasNextPage: false, endCursor: null } } };
  }
  if (query.includes("projects(")) return { projects: { nodes: projects } };
  if (query.includes("issue(")) return { issue: issues.find((i) => i.id === id) ?? null };
  return null;
}

export function startFakeLinear(port: number): Server {
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      res.setHeader("content-type", "application/json");
      if (req.headers.authorization?.trim() === "lin_api_bad") {
        // What Linear answers for a revoked or mistyped key.
        res.statusCode = 400;
        res.end(
          JSON.stringify({
            errors: [
              {
                message: "Authentication required, not authenticated",
                extensions: { type: "authentication error", code: "AUTHENTICATION_ERROR" },
              },
            ],
          }),
        );
        return;
      }
      const parsed = JSON.parse(body || "{}");
      const data = answer(parsed.query ?? "", parsed.variables ?? {});
      res.end(JSON.stringify(data ? { data } : { errors: [{ message: "Not faked" }] }));
    });
  });
  server.listen(port, "127.0.0.1");
  return server;
}

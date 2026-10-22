// A fake Linear GraphQL endpoint for the QA app (`TREQ_E2E_LINEAR_URL`):
// the viewer and a short issue list. Only a repository with a Linear API
// key reaches it, so views without one still show "not connected".
import { createServer, type Server } from "node:http";

const viewer = { id: "u-octocat", name: "octocat" };
const teammate = { id: "u-hubot", name: "hubot" };

function issue(
  n: number,
  title: string,
  state: [string, string],
  assignee: typeof viewer | null,
  priority: [number, string],
  project: string | null,
) {
  return {
    id: `issue-${n}`,
    identifier: `ENG-${n}`,
    title,
    description: `${title}.`,
    state: { name: state[0], type: state[1] },
    labels: { nodes: [] },
    branchName: `octocat/eng-${n}-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`,
    parent: null,
    children: { nodes: [] },
    url: `https://linear.app/lorem/issue/ENG-${n}`,
    assignee,
    priority: priority[0],
    priorityLabel: priority[1],
    project: project && { id: `p-${project}`, name: project },
  };
}

const issues = [
  issue(142, "Retry webhook deliveries", ["In Progress", "started"], viewer, [2, "High"], "Ingest"),
  issue(139, "Cache key-value lookups", ["Todo", "unstarted"], viewer, [3, "Medium"], null),
  issue(131, "Document the ingest runbook", ["Backlog", "backlog"], viewer, [0, "No priority"], "Docs"),
  issue(128, "Drop the v1 events table", ["Done", "completed"], viewer, [4, "Low"], null),
  issue(125, "Rate-limit the public API", ["Todo", "unstarted"], teammate, [1, "Urgent"], "API"),
];

export function startFakeLinear(port: number): Server {
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      const query: string = JSON.parse(body || "{}").query ?? "";
      const data = query.includes("viewer")
        ? { viewer }
        : query.includes("issues(")
          ? { issues: { nodes: issues, pageInfo: { hasNextPage: false, endCursor: null } } }
          : null;
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(data ? { data } : { errors: [{ message: "Not faked" }] }));
    });
  });
  server.listen(port, "127.0.0.1");
  return server;
}

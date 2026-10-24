// A fake Linear GraphQL endpoint for the QA app (`TREQ_E2E_LINEAR_URL`):
// the viewer, teams and a short issue list that issues can be created in,
// edited and deleted from. Only a repository with a Linear API key reaches
// it, so views without one still show "not connected".
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

const teams = [
  { id: "team-eng", name: "Engineering", key: "ENG" },
  { id: "team-des", name: "Design", key: "DES" },
];

let next = 143;

type Vars = {
  id?: string;
  input?: { teamId?: string; title?: string; description?: string };
};

/** The `data` for one GraphQL request, or `null` when it is not faked.
 * Issues created, edited and deleted here last for the QA run. */
function answer(query: string, variables: Vars): unknown {
  if (query.includes("viewer")) return { viewer };
  if (query.includes("teams")) return { teams: { nodes: teams } };
  if (query.includes("issueCreate(")) {
    const created = issue(
      next++,
      variables.input?.title ?? "",
      ["Todo", "unstarted"],
      null,
      [0, "No priority"],
      null,
    );
    created.description = variables.input?.description ?? "";
    issues.unshift(created);
    return { issueCreate: { success: true, issue: created } };
  }
  if (query.includes("issueUpdate(")) {
    const found = issues.find((i) => i.id === variables.id);
    if (found && variables.input?.title !== undefined) {
      found.title = variables.input.title;
      found.description = variables.input.description ?? found.description;
    }
    return { issueUpdate: { success: !!found, issue: found ?? null } };
  }
  if (query.includes("issueDelete(")) {
    const at = issues.findIndex((i) => i.id === variables.id);
    if (at >= 0) issues.splice(at, 1);
    return { issueDelete: { success: at >= 0 } };
  }
  if (query.includes("comments")) return { issue: { comments: { nodes: [] } } };
  if (query.includes("issues("))
    return { issues: { nodes: issues, pageInfo: { hasNextPage: false, endCursor: null } } };
  return null;
}

export function startFakeLinear(port: number): Server {
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      const request = JSON.parse(body || "{}");
      const data = answer(request.query ?? "", request.variables ?? {});
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(data ? { data } : { errors: [{ message: "Not faked" }] }));
    });
  });
  server.listen(port, "127.0.0.1");
  return server;
}

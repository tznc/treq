// A fake Supabase for the QA app when `QA_ACCOUNT=signed-in`: the debug
// build talks to the local project URL (package.json `env.dev`), so this
// listens there and answers what a signed-in Account tab reads. The session
// launch.ts stores refreshes into Ada's, on Pro with no cloud workspace.
// The first delete-account call refuses (the only owner of an organization
// with other members), later ones delete, so one run shows both answers.
import { createServer, type Server } from "node:http";

export const QA_USER = {
  id: "6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10",
  email: "ada@example.com",
  user_metadata: { full_name: "Ada Lovelace" },
};

export const SOLE_OWNER =
  "You are the only owner of Acme, which has other members. Make another member an owner or remove the other members, then delete your account.";

export function startFakeSupabase(port: number): Server {
  let deletions = 0;
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", `http://127.0.0.1:${port}`);
    const send = (status: number, body: unknown) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(status === 204 ? undefined : JSON.stringify(body));
    };
    req.resume();
    req.on("end", () => {
      const path = url.pathname;
      if (path === "/auth/v1/token") {
        return send(200, {
          access_token: "qa-access",
          refresh_token: "qa-refresh",
          expires_in: 86_400,
          user: QA_USER,
        });
      }
      if (path === "/auth/v1/logout") return send(204, null);
      if (path.startsWith("/auth/v1/user")) return send(200, QA_USER);
      if (path === "/rest/v1/subscriptions") {
        const row = { plan: "pro", status: "active", current_period_end: "2026-11-25T12:00:00Z" };
        const wantsObject = (req.headers.accept ?? "").includes("vnd.pgrst.object");
        return send(200, wantsObject ? row : [row]);
      }
      if (path === "/functions/v1/remote-instance") {
        return send(200, { instance: null, endpoint: null });
      }
      if (path === "/functions/v1/delete-account") {
        deletions += 1;
        return deletions === 1
          ? send(409, { error: SOLE_OWNER, code: "sole_owner" })
          : send(200, { deleted: true });
      }
      if (path.startsWith("/rest/v1/")) return send(200, []);
      return send(200, {});
    });
  });
  server.listen(port, "127.0.0.1");
  return server;
}

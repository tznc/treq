// One entry point for integrations, for the desktop app and for agents in
// cloud sandboxes. Routes (under /integration-proxy):
//
//   POST /sandbox-token   user only: { placement_id } -> { token, expires_at }
//   POST /linear/graphql  { query, variables } -> Linear's response
//   POST /github/token    { repository: "owner/name" } -> { token, expires_at }
//
// Every route takes `Authorization: Bearer <user's Supabase JWT | sandbox
// token from /sandbox-token>` (see lib.ts). verify_jwt is off in config.toml
// because sandbox tokens are not Supabase JWTs; lib.ts checks both.
// Env: INTEGRATION_PROXY_SECRET signs sandbox tokens.

import express from "npm:express@^4.21.2";
import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { appJWT } from "../_shared/github/app.ts";
import { linearTokenStore, proxyLinearRequest } from "../linear-proxy/lib.ts";
import {
  type Caller,
  exactIlike,
  githubRepoToken,
  mintSandboxJwt,
  resolveCaller,
} from "./lib.ts";

const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
const service = createClient(
  supabaseUrl,
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
);

const sandboxSecret = Deno.env.get("INTEGRATION_PROXY_SECRET") ?? "";

/** The owner of a placement that is not dead, or null. */
async function placementOwner(placementId: string): Promise<string | null> {
  const { data } = await service
    .from("remote_instance_placements")
    .select("owner_user_id")
    .eq("id", placementId)
    .neq("role", "dead")
    .maybeSingle();
  return (data?.owner_user_id as string | undefined) ?? null;
}

const app = express();
app.use(express.json({ limit: "1mb" }));

// Resolves the caller once per request; every route needs one.
app.use(async (req, res, next) => {
  try {
    const caller = await resolveCaller(req.headers.authorization, {
      sandboxSecret,
      userFromJwt: async (jwt) => {
        const { data, error } = await service.auth.getUser(jwt);
        return error ? null : data.user?.id ?? null;
      },
      placementOwner,
    });
    if (!caller) return res.status(401).json({ error: "Unauthorized" });
    res.locals.caller = caller;
    next();
  } catch (err) {
    next(err);
  }
});

const router = express.Router();

router.post("/sandbox-token", async (req, res) => {
  const caller = res.locals.caller as Caller;
  if (caller.kind !== "user") {
    return res.status(403).json({ error: "Sandbox tokens cannot mint tokens" });
  }
  const placementId = req.body?.placement_id;
  if (typeof placementId !== "string") {
    return res.status(400).json({ error: "Missing placement_id" });
  }
  if ((await placementOwner(placementId)) !== caller.userId) {
    return res.status(404).json({ error: "Placement not found" });
  }
  if (!sandboxSecret) return res.status(500).json({ error: "Sandbox tokens not configured" });
  const { token, expiresAt } = await mintSandboxJwt(sandboxSecret, placementId);
  res.json({ token, expires_at: new Date(expiresAt * 1000).toISOString() });
});

router.post("/linear/graphql", async (req, res) => {
  const caller = res.locals.caller as Caller;
  if (!req.body?.query) return res.status(400).json({ error: "Missing query" });
  const clientId = Deno.env.get("LINEAR_CLIENT_ID");
  const clientSecret = Deno.env.get("LINEAR_CLIENT_SECRET");
  const result = await proxyLinearRequest(
    { query: req.body.query, variables: req.body.variables },
    {
      fetch: fetchWithTimeout,
      linearClient: clientId && clientSecret
        ? { id: clientId, secret: clientSecret }
        : null,
      hasPro: () => userHasPro(service, caller.userId),
      store: linearTokenStore(service, caller.userId),
    },
  );
  res.status(result.status).type("application/json").send(result.body);
});

router.post("/github/token", async (req, res) => {
  const caller = res.locals.caller as Caller;
  const result = await githubRepoToken(caller.userId, req.body?.repository, {
    fetch: fetchWithTimeout,
    appJwt: appJWT,
    installationFor: async (userId, fullName) => {
      const { data } = await service
        .from("github_repositories")
        .select("installation_id, github_app_installations!inner(linked_user_id, suspended_at)")
        .ilike("full_name", exactIlike(fullName))
        .eq("github_app_installations.linked_user_id", userId)
        .is("github_app_installations.suspended_at", null)
        .limit(1)
        .maybeSingle();
      return (data?.installation_id as number | undefined) ?? null;
    },
    identityFor: async (userId) => {
      const { data } = await service
        .from("github_user_identities")
        .select("github_user_id, github_login")
        .eq("user_id", userId)
        .maybeSingle();
      return data
        ? { githubUserId: Number(data.github_user_id), login: data.github_login as string }
        : null;
    },
  });
  res.status(result.status).json(result.body);
});

app.use("/integration-proxy", router);

app.use(
  (err: unknown, _req: express.Request, res: express.Response, _next: express.NextFunction) => {
    console.error("[integration-proxy]", err instanceof Error ? err.message : String(err));
    res.status(500).json({ error: "Internal error" });
  },
);

app.listen(8000);

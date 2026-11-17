// One entry point for integrations, for the desktop and mobile apps and for
// agents in cloud sandboxes. Routes (under /integration-proxy):
//
//   POST /linear/graphql     apps: { query, variables } -> Linear's response
//   POST /github/user-token  apps: {} -> { token, expires_at }, acting as the user
//   POST /github/token       sandboxes: { repositories: ["owner/name", ...] }
//                            -> { tokens: [{ repositories, token, expires_at }], errors }
//
// Every route takes `Authorization: Bearer <user's Supabase JWT | sandbox
// token signed with the placement's integration secret>` (see lib.ts).
// verify_jwt is off in config.toml because sandbox tokens are not Supabase
// JWTs; lib.ts checks both.

import express from "npm:express@^4.21.2";
import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { appJWT } from "../_shared/github/app.ts";
import { appOnly, type Caller, exactIlike, resolveCaller } from "./lib.ts";
import { githubUserToken, type StoredGrant, type UserTokenDeps } from "./github-user-token.ts";
import { sandboxGithubTokens } from "./github-token.ts";
import { linearTokenStore, proxyLinearRequest } from "./linear.ts";

const service = createClient(
  Deno.env.get("SUPABASE_URL") ?? "",
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
);

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

/** A live placement's integration secret and owner, or null. */
async function placementSecret(placementId: string) {
  const ownerUserId = await placementOwner(placementId);
  if (!ownerUserId) return null;
  const { data } = await service.rpc("integration_secret_load", { p_placement_id: placementId });
  return typeof data === "string" && data ? { secret: data, ownerUserId } : null;
}

/** githubUserToken's dependencies for `userId`'s grant in Vault. */
function userTokenDeps(userId: string): UserTokenDeps {
  const clientId = Deno.env.get("GITHUB_APP_CLIENT_ID");
  const clientSecret = Deno.env.get("GITHUB_APP_CLIENT_SECRET");
  return {
    fetch: fetchWithTimeout,
    appClient: clientId && clientSecret ? { id: clientId, secret: clientSecret } : null,
    store: {
      load: async () => {
        const { data, error } = await service.rpc("github_user_tokens_load", {
          p_user_id: userId,
        });
        if (error) throw new Error(error.message);
        const row = (data as Record<string, string | null>[] | null)?.[0];
        return row
          ? ({
            accessToken: row.access_token as string,
            refreshToken: row.refresh_token,
            expiresAt: row.expires_at,
            refreshExpiresAt: row.refresh_expires_at,
            updatedAt: row.updated_at as string,
          } satisfies StoredGrant)
          : null;
      },
      save: async (grant) => {
        const { error } = await service.rpc("github_user_tokens_save", {
          p_user_id: userId,
          p_access_token: grant.accessToken,
          p_refresh_token: grant.refreshToken,
          p_expires_at: grant.expiresAt,
          p_refresh_expires_at: grant.refreshExpiresAt,
        });
        if (error) throw new Error(error.message);
      },
    },
  };
}

const app = express();
app.use(express.json({ limit: "1mb" }));

// Resolves the caller once per request; every route needs one.
app.use(async (req, res, next) => {
  try {
    const caller = await resolveCaller(req.headers.authorization, {
      placementSecret,
      userFromJwt: async (jwt) => {
        const { data, error } = await service.auth.getUser(jwt);
        return error ? null : data.user?.id ?? null;
      },
    });
    if (!caller) return res.status(401).json({ error: "Unauthorized" });
    res.locals.caller = caller;
    next();
  } catch (err) {
    next(err);
  }
});

const router = express.Router();

router.post("/linear/graphql", async (req, res) => {
  const caller = res.locals.caller as Caller;
  const refused = appOnly(caller);
  if (refused) return res.status(refused.status).json(refused.body);
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

router.post("/github/user-token", async (_req, res) => {
  const caller = res.locals.caller as Caller;
  const refused = appOnly(caller);
  if (refused) return res.status(refused.status).json(refused.body);
  const result = await githubUserToken(userTokenDeps(caller.userId));
  res.status(result.status).json(result.body);
});

router.post("/github/token", async (req, res) => {
  const caller = res.locals.caller as Caller;
  if (caller.kind !== "sandbox") {
    return res.status(403).json({
      error: "The apps use /github/user-token",
      code: "sandbox_only",
    });
  }
  const result = await sandboxGithubTokens(req.body?.repositories, {
    fetch: fetchWithTimeout,
    appJwt: appJWT,
    userToken: async () => {
      const issued = await githubUserToken(userTokenDeps(caller.userId));
      return issued.status === 200 ? (issued.body as { token: string }).token : null;
    },
    installationFor: async (fullName) => {
      const { data } = await service
        .from("github_repositories")
        .select("installation_id, github_app_installations!inner(linked_user_id, suspended_at)")
        .ilike("full_name", exactIlike(fullName))
        .eq("github_app_installations.linked_user_id", caller.userId)
        .is("github_app_installations.suspended_at", null)
        .limit(1)
        .maybeSingle();
      return (data?.installation_id as number | undefined) ?? null;
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

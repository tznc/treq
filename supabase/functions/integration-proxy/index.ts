// One entry point for integrations. Routes (under /integration-proxy):
//
//   POST /linear/graphql     { query, variables } -> Linear's response
//   POST /github/user-token  {} -> { token, expires_at }, acting as the user
//
// Every route takes `Authorization: Bearer <user's Supabase JWT>`.
// linear-proxy stays until desktop builds that call it are retired.

import express from "npm:express@^4.21.2";
import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { type Caller, resolveCaller } from "./lib.ts";
import { githubUserToken, type StoredGrant } from "./github-user-token.ts";
import { linearTokenStore, proxyLinearRequest } from "../linear-proxy/lib.ts";

const service = createClient(
  Deno.env.get("SUPABASE_URL") ?? "",
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
);

const app = express();
app.use(express.json({ limit: "1mb" }));

// Resolves the caller once per request; every route needs one.
app.use(async (req, res, next) => {
  try {
    const caller = await resolveCaller(req.headers.authorization, {
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
  const clientId = Deno.env.get("GITHUB_APP_CLIENT_ID");
  const clientSecret = Deno.env.get("GITHUB_APP_CLIENT_SECRET");
  const result = await githubUserToken({
    fetch: fetchWithTimeout,
    appClient: clientId && clientSecret ? { id: clientId, secret: clientSecret } : null,
    store: {
      load: async () => {
        const { data, error } = await service.rpc("github_user_tokens_load", {
          p_user_id: caller.userId,
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
          p_user_id: caller.userId,
          p_access_token: grant.accessToken,
          p_refresh_token: grant.refreshToken,
          p_expires_at: grant.expiresAt,
          p_refresh_expires_at: grant.refreshExpiresAt,
        });
        if (error) throw new Error(error.message);
      },
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

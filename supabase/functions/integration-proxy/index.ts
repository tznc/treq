// One entry point for integrations. Routes (under /integration-proxy):
//
//   POST /linear/graphql  { query, variables } -> Linear's response
//
// Every route takes `Authorization: Bearer <user's Supabase JWT>`.
// linear-proxy stays until desktop builds that call it are retired.

import express from "npm:express@^4.21.2";
import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import { type Caller, resolveCaller } from "./lib.ts";
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

app.use("/integration-proxy", router);

app.use(
  (err: unknown, _req: express.Request, res: express.Response, _next: express.NextFunction) => {
    console.error("[integration-proxy]", err instanceof Error ? err.message : String(err));
    res.status(500).json({ error: "Internal error" });
  },
);

app.listen(8000);

// Request logic for the remote-instance Edge Function (see index.ts for the
// actions), free of Deno globals so it runs under service-qa. index.ts
// verifies the user's JWT and supplies the service-role client, the
// provider, and the user's Pro check. The actions live in
// _shared/remote/instance-actions.ts; this file gates and dispatches.
//
// `ensure`, `wake` and `reprovision` require Pro (prds/billing-and-teams.md,
// "Enforcement"). `status` and `delete` always work, so a user whose Pro
// ended can still see their cloud workspace and remove it.

import { spendCapRefusal } from "../_shared/billing/spend-cap.ts";
import { PRO_REQUIRED_MESSAGES, refuseGatedAction } from "../_shared/billing/entitlement.ts";
import { REGION_CODES, SIZE_PRESETS } from "../_shared/remote/catalog.ts";
import { ProviderError, providerClientMessage } from "../_shared/remote/provider.ts";
import { BYOS_ACTIONS, handleByosAction, providerErrorCode } from "../_shared/remote/byos.ts";
import { acknowledgeMigration } from "../_shared/remote/migration-ack.ts";
import { handleProcessAction } from "../_shared/remote/process-actions.ts";
import { logWithCorrelation } from "../_shared/remote/correlation.ts";
import { rpc } from "../_shared/remote/db.ts";
import { json } from "../_shared/http/json.ts";
import {
  BYOS_DISABLED,
  handleDelete,
  handleEnsure,
  handleReprovision,
  handleResize,
  handleStatus,
  handleWake,
  HttpError,
  type RemoteInstanceDeps,
  type Reply,
} from "../_shared/remote/instance-actions.ts";

export type { RemoteInstanceDeps };

/** Actions that start or repair a machine. The rest stay open to Free users. */
export const PRO_GATED_ACTIONS: ReadonlySet<string> = new Set([
  "ensure",
  "wake",
  "reprovision",
  "resize",
  "acknowledge_migration",
]);

type Handler = (body: Record<string, unknown>, deps: RemoteInstanceDeps) => Promise<Reply>;

const processAction: Handler = (body, deps) =>
  handleProcessAction(body.action as "pin_process" | "unpin_process" | "kill_process", body, deps);

const ACTIONS: Record<string, Handler> = {
  list_regions: async () => ({ status: 200, body: { regions: REGION_CODES } }),
  list_sizes: async () => ({ status: 200, body: { presets: SIZE_PRESETS } }),
  catalog: async (_body, deps) => ({
    status: 200,
    body: { tiers: (await rpc(deps.supabase, "get_machine_catalog")) ?? [] },
  }),
  resize: handleResize,
  acknowledge_migration: (body, deps) => acknowledgeMigration({ ...deps, providers: deps.provider }, body),
  pin_process: processAction,
  unpin_process: processAction,
  kill_process: processAction,
  status: handleStatus,
  ensure: handleEnsure,
  wake: handleWake,
  reprovision: handleReprovision,
  delete: handleDelete,
};

const PROVIDER_ERROR_STATUS: Record<string, number> = {
  not_found: 404,
  already_exists: 409,
  quota_exceeded: 429,
  invalid_request: 400,
  timeout: 504,
  unauthorized: 409,
  unavailable: 502,
};

export async function handleRemoteInstanceAction(
  body: Record<string, unknown>,
  deps: RemoteInstanceDeps,
): Promise<Response> {
  const action = body.action;
  const reply = (r: Reply) => json(r.body, r.status, deps.correlationId);

  try {
    if (typeof action === "string" && BYOS_ACTIONS.has(action)) {
      return reply((deps.byos && await handleByosAction(action, body, deps.byos)) || BYOS_DISABLED);
    }
    const refused = await refuseGatedAction(
      action,
      PRO_GATED_ACTIONS,
      deps.hasPro,
      PRO_REQUIRED_MESSAGES.cloudWorkspace,
    );
    if (refused) return reply(refused);
    const capped = spendCapRefusal(action, deps.spendStatus ? await deps.spendStatus() : null);
    if (capped) return reply(capped);
    const handler = typeof action === "string" && Object.hasOwn(ACTIONS, action) ? ACTIONS[action] : null;
    if (!handler) return reply({ status: 400, body: { error: `Unknown action '${action}'` } });
    return reply(await handler(body, deps));
  } catch (err) {
    if (err instanceof HttpError) return reply({ status: err.status, body: { error: err.message, code: err.code } });
    const message = err instanceof ProviderError
      ? `provider ${err.kind}: ${err.message}`
      : `failed: ${(err as Error).message}`;
    logWithCorrelation(deps.correlationId, "error", `remote-instance action=${action} ${message}`);
    if (err instanceof ProviderError) {
      return reply({
        status: PROVIDER_ERROR_STATUS[err.kind] ?? 500,
        body: { error: providerClientMessage(err), code: providerErrorCode(err), provider_error: err.kind },
      });
    }
    return reply({ status: 500, body: { error: "Internal error", code: "internal_error" } });
  }
}

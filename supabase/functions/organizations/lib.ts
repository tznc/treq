// Request logic for the organizations Edge Function, free of Deno imports so
// it runs under the unit tests and service-qa. index.ts verifies the user's
// JWT and supplies an RPC function bound to the service role.
//
// Every rule (who may do what, token expiry and single use,
// last-owner protection, installation ownership, deleting an organization)
// lives in the SQL functions of 028_organizations_team.sql and
// 029_organization_owners.sql. This file checks the shape of the request,
// calls one function as the verified user, and turns its refusals into HTTP
// answers. After a join, leave or removal it updates the Team's billed seats
// (_shared/billing/team-seats.ts).

import { UUID_PATTERN } from "../_shared/billing/checkout.ts";
import type { HandlerResult } from "../_shared/intent-state.ts";

export type RpcError = {
  message: string;
  code?: string | null;
  hint?: string | null;
};

export type RpcResult = { data: unknown; error: RpcError | null };

export type OrganizationsDeps = {
  /** The user verified from the JWT. Every SQL function acts as them. */
  userId: string;
  rpc: (fn: string, args: Record<string, unknown>) => Promise<RpcResult>;
  /** Base URL for invite links, for example https://treq.dev. */
  webUrl: string;
  /**
   * Sets the organization's Team quantity to its member count. Missing when
   * Stripe is not configured. A failure is logged, never returned: the
   * membership change has already committed.
   */
  syncSeats?: (organizationId: string) => Promise<unknown>;
};

/**
 * The link an owner shares. The token in it is the whole invite, so it goes
 * in the fragment, which browsers never send to a server. The dashboard
 * reads it from location.hash and then clears it from the address bar.
 */
export function inviteAcceptUrl(webUrl: string, token: string): string {
  return `${webUrl.replace(/\/+$/, "")}/dashboard?tab=team#invite=${token}`;
}

type Fields = Record<string, unknown>;

type Call = {
  fn: string;
  args: Record<string, unknown>;
  respond: (data: unknown) => Record<string, unknown>;
  /** The organization whose member count this call changed, if any. */
  seatsChanged?: (data: unknown) => string | null;
};

function uuid(fields: Fields, key: string): string | null {
  const value = fields[key];
  return typeof value === "string" && UUID_PATTERN.test(value)
    ? value.toLowerCase()
    : null;
}

function text(fields: Fields, key: string): string | null {
  const value = fields[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

const ok = () => ({ ok: true });

// Returns the SQL call for a request, or the reason it is malformed.
function callFor(
  fields: Fields,
  userId: string,
  webUrl: string,
): Call | string {
  const actor = { p_actor: userId };
  switch (fields.action) {
    case "create": {
      const name = text(fields, "name");
      if (name === null) return "create needs a name";
      return {
        fn: "organization_create",
        args: { ...actor, p_name: name },
        respond: (data) => ({ organization: data }),
      };
    }
    case "invite": {
      const orgId = uuid(fields, "organization_id");
      const email = text(fields, "email");
      if (orgId === null || email === null) {
        return "invite needs organization_id and email";
      }
      return {
        fn: "organization_invite",
        args: { ...actor, p_org_id: orgId, p_email: email },
        respond: (data) => {
          const invite = data as {
            invite_id: string;
            email: string;
            expires_at: string;
            token: string;
          };
          return {
            invite: {
              id: invite.invite_id,
              email: invite.email,
              expires_at: invite.expires_at,
            },
            accept_url: inviteAcceptUrl(webUrl, invite.token),
          };
        },
      };
    }
    case "accept": {
      const token = text(fields, "token");
      if (token === null) return "accept needs a token";
      return {
        fn: "organization_accept_invite",
        args: { ...actor, p_token: token },
        respond: (data) => {
          const joined = data as {
            organization_id: string;
            name: string;
            result: string;
          };
          return {
            organization: { id: joined.organization_id, name: joined.name },
            result: joined.result,
          };
        },
        seatsChanged: (data) => {
          const joined = data as { organization_id: string; result: string };
          return joined.result === "joined" ? joined.organization_id : null;
        },
      };
    }
    case "revoke_invite": {
      const inviteId = uuid(fields, "invite_id");
      if (inviteId === null) return "revoke_invite needs invite_id";
      return {
        fn: "organization_revoke_invite",
        args: { ...actor, p_invite_id: inviteId },
        respond: ok,
      };
    }
    case "remove_member": {
      const orgId = uuid(fields, "organization_id");
      const memberId = uuid(fields, "user_id");
      if (orgId === null || memberId === null) {
        return "remove_member needs organization_id and user_id";
      }
      return {
        fn: "organization_remove_member",
        args: { ...actor, p_org_id: orgId, p_user_id: memberId },
        respond: ok,
        seatsChanged: () => orgId,
      };
    }
    case "leave": {
      const orgId = uuid(fields, "organization_id");
      if (orgId === null) return "leave needs organization_id";
      return {
        fn: "organization_leave",
        args: { ...actor, p_org_id: orgId },
        respond: ok,
        seatsChanged: () => orgId,
      };
    }
    case "promote_member":
    case "demote_owner": {
      const orgId = uuid(fields, "organization_id");
      const memberId = uuid(fields, "user_id");
      if (orgId === null || memberId === null) {
        return `${fields.action} needs organization_id and user_id`;
      }
      return {
        fn:
          fields.action === "promote_member"
            ? "organization_promote_member"
            : "organization_demote_owner",
        args: { ...actor, p_org_id: orgId, p_user_id: memberId },
        respond: (data) => ({ ok: true, result: data }),
      };
    }
    case "delete_organization": {
      const orgId = uuid(fields, "organization_id");
      if (orgId === null) return "delete_organization needs organization_id";
      return {
        fn: "organization_delete",
        args: { ...actor, p_org_id: orgId },
        respond: ok,
      };
    }
    case "attach_installation": {
      const orgId = uuid(fields, "organization_id");
      const installationId = fields.installation_id;
      if (
        orgId === null ||
        typeof installationId !== "number" ||
        !Number.isSafeInteger(installationId) ||
        installationId <= 0
      ) {
        return "attach_installation needs organization_id and installation_id";
      }
      return {
        fn: "organization_attach_installation",
        args: { ...actor, p_org_id: orgId, p_installation_id: installationId },
        respond: (data) => ({ ok: true, result: data }),
      };
    }
    default:
      return "action must be one of create, invite, accept, revoke_invite, remove_member, leave, promote_member, demote_owner, delete_organization, attach_installation";
  }
}

// The SQL functions refuse with SQLSTATE PTnnn, meaning HTTP nnn, a message
// written for the dashboard, and a stable code in the hint.
const REFUSAL = /^PT(4\d\d)$/;

export async function handleOrganizationsRequest(
  body: unknown,
  deps: OrganizationsDeps,
): Promise<HandlerResult> {
  const fields: Fields =
    typeof body === "object" && body !== null ? (body as Fields) : {};
  const call = callFor(fields, deps.userId, deps.webUrl);
  if (typeof call === "string") return { status: 400, body: { error: call } };

  const { data, error } = await deps.rpc(call.fn, call.args);
  if (error) {
    const refusal = REFUSAL.exec(error.code ?? "");
    if (refusal) {
      return {
        status: Number(refusal[1]),
        body: { error: error.message, code: error.hint ?? null },
      };
    }
    console.error(
      JSON.stringify({
        function: "organizations",
        level: "error",
        action: fields.action,
        error: error.message,
      }),
    );
    return { status: 500, body: { error: "Organization request failed" } };
  }

  const changedOrg = call.seatsChanged?.(data) ?? null;
  if (changedOrg && deps.syncSeats) {
    try {
      await deps.syncSeats(changedOrg);
    } catch (err) {
      console.error(
        JSON.stringify({
          function: "organizations",
          level: "error",
          action: fields.action,
          message: "Team seat sync failed",
          organization_id: changedOrg,
          error: err instanceof Error ? err.message : String(err),
        }),
      );
    }
  }
  return { status: 200, body: call.respond(data) };
}

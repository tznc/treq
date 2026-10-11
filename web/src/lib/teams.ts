// The dashboard's side of organizations and Team (prds/billing-and-teams.md,
// "Organizations and seats"). The organizations Edge Function and the SQL
// behind it enforce every rule. These helpers read invite links, carry an
// invite through sign-in, and count seats for display.

/** Team's flat monthly price covers this many members. */
export const TEAM_INCLUDED_SEATS = 5;
/** Each member beyond the included seats adds this much per month. */
export const TEAM_EXTRA_SEAT_USD = 8;

const TOKEN_PATTERN = /^[0-9a-f]{64}$/;

function validToken(value: string | null): string | null {
  return value && TOKEN_PATTERN.test(value) ? value : null;
}

// Invite links carry the token in the URL fragment,
// /dashboard?tab=team#invite=<token>. Browsers never send a fragment to a
// server, so the token stays out of request logs and the page URL that
// analytics records on load, and the dashboard removes it from the address
// bar as soon as it reads it.

/** The invite token in a URL fragment such as `#invite=<token>`. */
export function inviteTokenFromHash(hash: string): string | null {
  return validToken(new URLSearchParams(hash.replace(/^#/, "")).get("invite"));
}

/**
 * The invite token in a pasted invite link, or a pasted bare token. Accepts
 * the fragment form and the older `?invite=<token>` query form.
 */
export function inviteTokenFromInput(value: string): string | null {
  const trimmed = value.trim();
  const bare = trimmed.toLowerCase();
  if (TOKEN_PATTERN.test(bare)) return bare;
  try {
    const url = new URL(trimmed);
    return inviteTokenFromHash(url.hash) ?? validToken(url.searchParams.get("invite"));
  } catch {
    return null;
  }
}

/** The sessionStorage key that carries an invite token through sign-in. */
export const INVITE_STASH_KEY = "treq.teamInvite";

export type InviteStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

/**
 * What reading an invite needs from `window`. `sessionStorage` is a getter
 * because reading `window.sessionStorage` itself throws when site data is
 * blocked.
 */
export type InvitePage = {
  location: Pick<Location, "pathname" | "search" | "hash">;
  history: Pick<History, "state" | "replaceState">;
  sessionStorage: () => InviteStorage;
};

export function invitePage(): InvitePage {
  return {
    location: window.location,
    history: window.history,
    sessionStorage: () => window.sessionStorage,
  };
}

/**
 * Called before a signed-out visitor is sent to /sign-in. Its `redirect`
 * carries the path and query only, so the token waits in this tab's
 * sessionStorage. Without storage (a private window, blocked site data) the
 * invite is simply lost and the link has to be opened again.
 */
export function stashInviteForSignIn(page: InvitePage): void {
  const token = inviteTokenFromHash(page.location.hash);
  if (!token) return;
  try {
    page.sessionStorage().setItem(INVITE_STASH_KEY, token);
  } catch {
    // Storage unavailable: nothing to keep the token in.
  }
}

/**
 * The invite this page was opened with: the link's fragment, or else a
 * token stashed before sign-in. Only reads; see forgetPendingInvite.
 */
export function pendingInviteToken(page: InvitePage): string | null {
  const fromHash = inviteTokenFromHash(page.location.hash);
  if (fromHash) return fromHash;
  try {
    return validToken(page.sessionStorage().getItem(INVITE_STASH_KEY));
  } catch {
    return null;
  }
}

/** Removes a read invite from the address bar and from the stash. */
export function forgetPendingInvite(page: InvitePage): void {
  const { pathname, search, hash } = page.location;
  if (new URLSearchParams(hash.replace(/^#/, "")).has("invite")) {
    page.history.replaceState(page.history.state, "", `${pathname}${search}`);
  }
  try {
    page.sessionStorage().removeItem(INVITE_STASH_KEY);
  } catch {
    // Storage unavailable: nothing was stashed.
  }
}

/** Members billed beyond the included seats. Pending invites are not billed. */
export function extraSeats(members: number): number {
  return Math.max(0, members - TEAM_INCLUDED_SEATS);
}

export type OrganizationAction =
  | { action: "create"; name: string }
  | { action: "invite"; organization_id: string; email: string }
  | { action: "accept"; token: string }
  | { action: "revoke_invite"; invite_id: string }
  | { action: "remove_member"; organization_id: string; user_id: string }
  | { action: "leave"; organization_id: string }
  | { action: "promote_member"; organization_id: string; user_id: string }
  | { action: "demote_owner"; organization_id: string; user_id: string }
  | { action: "delete_organization"; organization_id: string }
  | { action: "attach_installation"; organization_id: string; installation_id: number };

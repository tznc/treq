import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

type AuthCallbackUrlOptions = {
  siteUrl: string;
  browserOrigin: string;
  isProduction: boolean;
  query?: string | undefined;
};

/** Build an auth callback URL without relying on the deployment server's origin. */
export function getAuthCallbackUrl({
  siteUrl,
  browserOrigin,
  isProduction,
  query,
}: AuthCallbackUrlOptions): string {
  const origin = (isProduction ? siteUrl : browserOrigin).replace(/\/$/, "");
  return `${origin}/auth/callback${query ? `?${query}` : ""}`;
}

const DEFAULT_REDIRECT_PATH = "/dashboard";

const REDIRECT_CHECK_ORIGIN = "https://redirect-check.invalid";

/**
 * Return `target` as a path on this site, or `/dashboard` when it is not one.
 * Only a path with one leading `/` passes, so `//host`, `https:` and
 * `javascript:` targets cannot send a user off the site after sign-in.
 */
export function safeRedirectPath(target: string | null | undefined): string {
  if (
    !target ||
    !target.startsWith("/") ||
    target.startsWith("//") ||
    target.includes("\\") ||
    // Browsers drop tabs and newlines from URLs, so "/\t/evil.com" would
    // become "//evil.com".
    /[\u0000-\u001f\u007f]/.test(target)
  ) {
    return DEFAULT_REDIRECT_PATH;
  }
  // Resolve against a fixed origin as a final check that the host cannot change.
  const url = new URL(target, REDIRECT_CHECK_ORIGIN);
  if (url.origin !== REDIRECT_CHECK_ORIGIN) return DEFAULT_REDIRECT_PATH;
  return `${url.pathname}${url.search}${url.hash}`;
}

/**
 * Query string /sign-in adds to the OAuth callback URL. `redirect` is only
 * carried when the caller asked for one, and it is always the checked path.
 */
export function signInCallbackQuery(
  source: string,
  requestedRedirect: string | null,
): string | undefined {
  const query = new URLSearchParams();
  if (source) query.set("source", source);
  if (requestedRedirect !== null) query.set("redirect", safeRedirectPath(requestedRedirect));
  return query.toString() || undefined;
}

/**
 * Where /auth/callback sends a signed-in user. Only the desktop app needs a
 * one-time token, so web sign-ins go straight to the checked redirect path.
 */
export function authCallbackDestination(
  params: URLSearchParams,
): { kind: "desktop" } | { kind: "web"; path: string } {
  if (params.get("source") === "desktop") return { kind: "desktop" };
  return { kind: "web", path: safeRedirectPath(params.get("redirect")) };
}

/** Sign-in link that returns the user to `pathAndSearch` afterwards. */
export function signInHrefFor(pathAndSearch: string): string {
  return `/sign-in?redirect=${encodeURIComponent(pathAndSearch)}`;
}

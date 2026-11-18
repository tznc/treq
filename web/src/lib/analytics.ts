// gtag only exists when the site is built with DOCUSAURUS_ENABLE_GTAG=true.
export function trackEvent(name: string, params: Record<string, string>): void {
  const gtag = (window as {gtag?: (...args: unknown[]) => void}).gtag;
  if (typeof gtag === 'function') gtag('event', name, params);
}

type AuthUser = {
  created_at?: string | null;
  email_confirmed_at?: string | null;
  last_sign_in_at?: string | null;
};

// Supabase sets last_sign_in_at on the first session: at account creation for
// OAuth, at confirmation for email. A later sign-in is minutes or days after.
const FIRST_SIGN_IN_WINDOW_MS = 60_000;

export function isFirstSignIn(user: AuthUser): boolean {
  const start = user.email_confirmed_at ?? user.created_at;
  if (!start || !user.last_sign_in_at) return false;
  return Date.parse(user.last_sign_in_at) - Date.parse(start) < FIRST_SIGN_IN_WINDOW_MS;
}

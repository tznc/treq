export type ConsentChoice = 'granted' | 'denied';

// Keep in sync with CONSENT_STORAGE_KEY in plugins/googleTagPlugin.js.
export const CONSENT_STORAGE_KEY = 'treq-analytics-consent';
export const OPEN_CONSENT_EVENT = 'treq:open-consent';

// Browsers do not expose the visitor's country, so the banner uses the time
// zone. Google applies the denied default by IP region regardless, so a
// mismatch only means a visitor is asked who did not need to be, or sends no
// analytics until they are.
export function shouldAskForConsent(timeZone: string, stored: string | null): boolean {
  if (stored === 'granted' || stored === 'denied') return false;
  return timeZone.startsWith('Europe/') || /^Atlantic\/(Canary|Madeira|Azores|Reykjavik|Faroe)$/.test(timeZone);
}

export function readConsent(): string | null {
  try {
    return localStorage.getItem(CONSENT_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function saveConsent(choice: ConsentChoice): void {
  try {
    localStorage.setItem(CONSENT_STORAGE_KEY, choice);
  } catch {
    // Storage blocked: the choice applies to this page view only.
  }
  const gtag = (window as {gtag?: (...args: unknown[]) => void}).gtag;
  if (typeof gtag === 'function') gtag('consent', 'update', {analytics_storage: choice});
}

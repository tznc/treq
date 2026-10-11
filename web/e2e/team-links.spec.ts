import { expect, test } from '@playwright/test';
import {
  extraSeats,
  forgetPendingInvite,
  INVITE_STASH_KEY,
  type InvitePage,
  type InviteStorage,
  inviteTokenFromHash,
  inviteTokenFromInput,
  pendingInviteToken,
  stashInviteForSignIn,
} from '../src/lib/teams';
import { safeRedirectPath } from '../src/lib/utils';

// Pure logic behind the Team tab's invite links and the sign-in round trip
// that brings an invited user back to them. Invite links carry the token in
// the URL fragment, so it never reaches a server or a recorded page URL.
const TOKEN = '0123456789abcdef'.repeat(4);

function memoryStorage(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial));
  const storage: InviteStorage = {
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, value),
    removeItem: (key) => void items.delete(key),
  };
  return { items, storage };
}

const blockedStorage = (): InviteStorage => {
  throw new DOMException('The operation is insecure.', 'SecurityError');
};

function fakePage(url: string, storage: () => InviteStorage) {
  const parsed = new URL(url, 'https://treq.dev');
  const replaced: string[] = [];
  const page: InvitePage = {
    location: { pathname: parsed.pathname, search: parsed.search, hash: parsed.hash },
    history: {
      state: { key: 'docusaurus' },
      replaceState: (_state, _unused, next) => void replaced.push(String(next)),
    },
    sessionStorage: storage,
  };
  return { page, replaced };
}

test('reads an invite token from the URL fragment', () => {
  expect(inviteTokenFromHash(`#invite=${TOKEN}`)).toBe(TOKEN);
  expect(inviteTokenFromHash('')).toBeNull();
  expect(inviteTokenFromHash('#invite=not-a-token')).toBeNull();
  expect(inviteTokenFromHash(`#other=${TOKEN}`)).toBeNull();
});

test('reads an invite token from a pasted link in either form, or a bare token', () => {
  expect(inviteTokenFromInput(`https://treq.dev/dashboard?tab=team#invite=${TOKEN}`)).toBe(TOKEN);
  expect(inviteTokenFromInput(`https://treq.dev/dashboard?tab=team&invite=${TOKEN}`)).toBe(TOKEN);
  expect(inviteTokenFromInput(`  ${TOKEN}  `)).toBe(TOKEN);
  expect(inviteTokenFromInput(`${TOKEN.toUpperCase()}`)).toBe(TOKEN);
  expect(inviteTokenFromInput('https://treq.dev/dashboard?tab=team')).toBeNull();
  expect(inviteTokenFromInput('https://treq.dev/dashboard?tab=team#invite=nope')).toBeNull();
  expect(inviteTokenFromInput('hello')).toBeNull();
});

test('an opened invite link is read, then cleared from the address bar', () => {
  const { items, storage } = memoryStorage();
  const { page, replaced } = fakePage(`/dashboard?tab=team#invite=${TOKEN}`, () => storage);

  expect(pendingInviteToken(page)).toBe(TOKEN);
  forgetPendingInvite(page);
  expect(replaced).toEqual(['/dashboard?tab=team']);
  expect(items.size).toBe(0);
});

test('a signed-out visitor keeps the invite through sign-in', () => {
  const { items, storage } = memoryStorage();
  // Before redirecting to /sign-in?redirect=/dashboard?tab=team, which
  // carries no fragment.
  const before = fakePage(`/dashboard?tab=team#invite=${TOKEN}`, () => storage);
  stashInviteForSignIn(before.page);
  expect(items.get(INVITE_STASH_KEY)).toBe(TOKEN);

  // Back on the dashboard after sign-in, without the fragment.
  const after = fakePage('/dashboard?tab=team', () => storage);
  expect(pendingInviteToken(after.page)).toBe(TOKEN);
  forgetPendingInvite(after.page);
  expect(after.replaced).toEqual([]);
  expect(items.has(INVITE_STASH_KEY)).toBe(false);
  expect(pendingInviteToken(after.page)).toBeNull();
});

test('the fragment wins over an older stashed invite, and junk is ignored', () => {
  const OTHER = 'f'.repeat(64);
  const { storage } = memoryStorage({ [INVITE_STASH_KEY]: OTHER });
  expect(pendingInviteToken(fakePage(`/dashboard#invite=${TOKEN}`, () => storage).page)).toBe(TOKEN);
  expect(pendingInviteToken(fakePage('/dashboard', () => storage).page)).toBe(OTHER);

  const junk = memoryStorage({ [INVITE_STASH_KEY]: '<script>' });
  expect(pendingInviteToken(fakePage('/dashboard', () => junk.storage).page)).toBeNull();

  const noInvite = memoryStorage();
  const { page, replaced } = fakePage('/dashboard?tab=team#section', () => noInvite.storage);
  stashInviteForSignIn(page);
  expect(noInvite.items.size).toBe(0);
  forgetPendingInvite(page);
  expect(replaced).toEqual([]);
});

test('blocked storage does not break invite links', () => {
  const { page, replaced } = fakePage(`/dashboard?tab=team#invite=${TOKEN}`, blockedStorage);
  expect(() => stashInviteForSignIn(page)).not.toThrow();
  expect(pendingInviteToken(page)).toBe(TOKEN);
  expect(() => forgetPendingInvite(page)).not.toThrow();
  expect(replaced).toEqual(['/dashboard?tab=team']);
  expect(pendingInviteToken(fakePage('/dashboard', blockedStorage).page)).toBeNull();
});

test('bills members beyond the five included seats, not pending invites', () => {
  expect(extraSeats(3)).toBe(0);
  expect(extraSeats(5)).toBe(0);
  expect(extraSeats(8)).toBe(3);
});

test('keeps sign-in redirects on this site', () => {
  expect(safeRedirectPath('/dashboard?tab=team')).toBe('/dashboard?tab=team');
  expect(safeRedirectPath('https://evil.example/dashboard')).toBe('/dashboard');
  expect(safeRedirectPath('//evil.example/dashboard')).toBe('/dashboard');
  expect(safeRedirectPath('/\\evil.example')).toBe('/dashboard');
  expect(safeRedirectPath('javascript:alert(1)')).toBe('/dashboard');
  expect(safeRedirectPath(null)).toBe('/dashboard');
});

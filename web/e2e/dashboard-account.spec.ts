import { expect, test, type Browser, type Page } from '@playwright/test';
import pkg from '../../package.json';

// Deleting the account from /dashboard against the production build, with
// Supabase stubbed at the network layer: the email must be typed before the
// button works, a refusal shows the server's sentence, and a deletion signs
// out and leaves the dashboard.

const user = {
  id: '6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10',
  aud: 'authenticated',
  role: 'authenticated',
  email: 'delete-e2e@treq.dev',
  app_metadata: { provider: 'email' },
  user_metadata: { full_name: 'Delete E2E' },
  created_at: '2026-10-01T00:00:00Z',
};

const SOLE_OWNER =
  'You are the only owner of Acme, which has other members. Make another member an owner or remove the other members, then delete your account.';

function fakeSession() {
  const now = Math.floor(Date.now() / 1000);
  return {
    access_token: [
      Buffer.from(JSON.stringify({ alg: 'none', typ: 'JWT' })).toString('base64url'),
      Buffer.from(
        JSON.stringify({ sub: user.id, email: user.email, role: 'authenticated', exp: now + 3600 }),
      ).toString('base64url'),
      'e2e',
    ].join('.'),
    refresh_token: 'e2e-refresh-token',
    expires_in: 3600,
    expires_at: now + 3600,
    token_type: 'bearer',
    user,
  };
}

async function openDashboard(
  browser: Browser,
  deletion: { status: number; body: unknown },
  path = '/dashboard?tab=subscription',
): Promise<{ page: Page; calls: () => number; close: () => Promise<void> }> {
  const session = fakeSession();
  const ref = new URL(pkg.env.prod.supabase.url).hostname.split('.')[0];
  const context = await browser.newContext({
    baseURL: 'http://localhost:3000',
    storageState: {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:3000',
          localStorage: [{ name: `sb-${ref}-auth-token`, value: JSON.stringify(session) }],
        },
      ],
    },
  });
  const page = await context.newPage();
  let deletions = 0;

  await page.route(/\/auth\/v1\//, (route) =>
    route.fulfill({
      status: route.request().url().includes('/logout') ? 204 : 200,
      contentType: 'application/json',
      body: JSON.stringify(route.request().url().includes('/token') ? session : user),
    }),
  );
  await page.route(/\/rest\/v1\//, (route) => {
    const wantsObject = (route.request().headers()['accept'] ?? '').includes('vnd.pgrst.object');
    const free = { plan: 'free', status: 'inactive', current_period_end: null };
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(route.request().url().includes('/subscriptions') && wantsObject ? free : []),
    });
  });
  await page.route(/\/functions\/v1\/delete-account/, (route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204 });
    deletions += 1;
    return route.fulfill({
      status: deletion.status,
      contentType: 'application/json',
      body: JSON.stringify(deletion.body),
    });
  });

  await page.goto(path);
  return { page, calls: () => deletions, close: () => context.close() };
}

const section = (page: Page) => page.getByRole('region', { name: 'Delete account' });

test.describe('Dashboard account deletion', () => {
  test('a refusal keeps the account and shows the server sentence', async ({ browser }) => {
    const { page, calls, close } = await openDashboard(browser, {
      status: 409,
      body: { error: SOLE_OWNER, code: 'sole_owner' },
    });
    await section(page).getByRole('button', { name: 'Delete account…' }).click();
    const confirm = section(page).getByRole('button', { name: 'Delete account', exact: true });
    await expect(confirm).toBeDisabled();
    await section(page).getByLabel(`Type ${user.email} to confirm`).fill('someone@treq.dev');
    await expect(confirm).toBeDisabled();
    await section(page).getByLabel(`Type ${user.email} to confirm`).fill(user.email);
    await confirm.click();
    await expect(section(page).getByRole('alert')).toHaveText(SOLE_OWNER);
    expect(calls()).toBe(1);
    await expect(section(page).getByLabel(`Type ${user.email} to confirm`)).toBeVisible();
    await close();
  });

  test('a deletion signs out and leaves the dashboard', async ({ browser }) => {
    const { page, calls, close } = await openDashboard(browser, {
      status: 200,
      body: { deleted: true },
    });
    await section(page).getByRole('button', { name: 'Delete account…' }).click();
    await section(page).getByLabel(`Type ${user.email} to confirm`).fill(user.email);
    await section(page).getByRole('button', { name: 'Delete account', exact: true }).click();
    await expect(section(page)).toHaveCount(0);
    expect(calls()).toBe(1);
    await close();
  });
});

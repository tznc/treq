import { expect, test, type Browser, type Page } from '@playwright/test';
import pkg from '../../package.json';
import { STRIPE_PUBLISHABLE_KEY_PROD } from '../src/lib/constants';

// The Subscription tab of /dashboard against the production build, with
// Supabase and Stripe stubbed at the network layer. Checkout opens only when
// the stripePayments flag is on and the live publishable key is set, so the
// checkout test runs once both ship and the disabled-button test runs until
// then.
const checkoutLive =
  Boolean(pkg.featureFlags.stripePayments) && STRIPE_PUBLISHABLE_KEY_PROD !== '';

type SubscriptionRow = {
  plan: string;
  status: string;
  current_period_end: string | null;
};

const FREE: SubscriptionRow = { plan: 'free', status: 'inactive', current_period_end: null };

const user = {
  id: '6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10',
  aud: 'authenticated',
  role: 'authenticated',
  email: 'billing-e2e@treq.dev',
  app_metadata: { provider: 'email' },
  user_metadata: { full_name: 'Billing E2E' },
  created_at: '2026-10-01T00:00:00Z',
};

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

// Stands in for Stripe.js: initEmbeddedCheckout fetches the client secret
// through our billing-checkout function and "mounts" by writing it out.
const FAKE_STRIPE_JS = `
window.Stripe = function (key) {
  return {
    initEmbeddedCheckout: async function (options) {
      const secret = await options.fetchClientSecret();
      return {
        mount: function (target) {
          const node = typeof target === 'string' ? document.querySelector(target) : target;
          node.textContent = 'Embedded Checkout ' + key + ' ' + secret;
        },
        unmount: function () {},
        destroy: function () {},
      };
    },
  };
};
`;

type Stubs = {
  subscriptions: SubscriptionRow[];
  billingCustomer?: { trial_used_at: string | null } | null;
  portalStatus?: number;
};

type Calls = { checkout: unknown[]; portal: number; subscriptionReads: number };

async function openDashboard(
  browser: Browser,
  stubs: Stubs,
  path = '/dashboard?tab=subscription',
): Promise<{ page: Page; calls: Calls; close: () => Promise<void> }> {
  const session = fakeSession();
  const ref = new URL(pkg.env.prod.supabase.url).hostname.split('.')[0];
  const context = await browser.newContext({
    baseURL: 'http://localhost:3000',
    storageState: {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:3000',
          // The E2E server serves a production build, so Supabase reads the
          // storage key derived from the production project URL.
          localStorage: [{ name: `sb-${ref}-auth-token`, value: JSON.stringify(session) }],
        },
      ],
    },
  });
  const page = await context.newPage();
  const calls: Calls = { checkout: [], portal: 0, subscriptionReads: 0 };

  await page.route(/\/auth\/v1\//, (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(route.request().url().includes('/token') ? session : user),
    }),
  );

  // supabase-js asks for one object (`.single()`) or an array (`.maybeSingle()`)
  // through the Accept header.
  const rows = (route: Parameters<Parameters<Page['route']>[1]>[0], list: unknown[]) => {
    const wantsObject = (route.request().headers()['accept'] ?? '').includes('vnd.pgrst.object');
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(wantsObject ? list[0] : list),
    });
  };
  await page.route(/\/rest\/v1\/subscriptions/, (route) => {
    const index = Math.min(calls.subscriptionReads, stubs.subscriptions.length - 1);
    calls.subscriptionReads += 1;
    return rows(route, [stubs.subscriptions[index]]);
  });
  await page.route(/\/rest\/v1\/billing_customers/, (route) =>
    rows(route, stubs.billingCustomer ? [stubs.billingCustomer] : []),
  );
  await page.route(/\/functions\/v1\/billing-checkout/, async (route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204 });
    calls.checkout.push(route.request().postDataJSON());
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ client_secret: 'cs_test_e2e_secret' }),
    });
  });
  await page.route(/\/functions\/v1\/billing-portal/, async (route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204 });
    calls.portal += 1;
    const status = stubs.portalStatus ?? 200;
    return route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(
        status === 200
          ? { url: 'https://billing.stripe.com/p/session/e2e' }
          : { error: 'No billing account for this user' },
      ),
    });
  });
  await page.route('https://billing.stripe.com/**', (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: '<h1>Stripe customer portal</h1>' }),
  );
  await page.route('https://js.stripe.com/**', (route) =>
    route.fulfill({ status: 200, contentType: 'application/javascript', body: FAKE_STRIPE_JS }),
  );

  await page.goto(path);
  return { page, calls, close: () => context.close() };
}

const card = (page: Page) => page.getByRole('region', { name: 'Subscription' });

function field(page: Page, label: string) {
  return card(page).getByRole('group', { name: label });
}

test.describe('Dashboard subscription tab', () => {
  test('a trialing user sees Pro, the trial end and Manage billing', async ({ browser }) => {
    const { page, calls, close } = await openDashboard(browser, {
      subscriptions: [{ plan: 'pro', status: 'trialing', current_period_end: '2026-10-19T12:00:00' }],
      billingCustomer: { trial_used_at: '2026-10-05T12:00:00Z' },
    });

    await expect(field(page, 'Current Plan')).toContainText('Pro');
    await expect(field(page, 'Status')).toContainText('Trial');
    await expect(field(page, 'Trial ends')).toContainText('2026');
    await expect(card(page).getByRole('button', { name: /upgrade|trial/i })).toHaveCount(0);

    await card(page).getByRole('button', { name: 'Manage billing' }).click();
    await expect(page.getByRole('heading', { name: 'Stripe customer portal' })).toBeVisible();
    expect(calls.portal).toBe(1);
    await close();
  });

  test('an active user sees the renewal date', async ({ browser }) => {
    const { page, close } = await openDashboard(browser, {
      subscriptions: [{ plan: 'pro', status: 'active', current_period_end: '2026-11-19T12:00:00' }],
      billingCustomer: { trial_used_at: null },
    });
    await expect(field(page, 'Status')).toContainText('Active');
    await expect(field(page, 'Renews on')).toContainText('2026');
    await close();
  });

  test('a subscription set to cancel stays Pro and says when it ends', async ({ browser }) => {
    const { page, close } = await openDashboard(browser, {
      subscriptions: [{ plan: 'pro', status: 'canceled', current_period_end: '2026-11-19T12:00:00' }],
      billingCustomer: { trial_used_at: null },
    });
    await expect(field(page, 'Current Plan')).toContainText('Pro');
    await expect(field(page, 'Status')).toContainText('Canceling');
    await expect(field(page, 'Pro until')).toContainText('2026');
    await expect(card(page)).toContainText('set to cancel');
    await expect(card(page).getByRole('button', { name: 'Manage billing' })).toBeVisible();
    await close();
  });

  test('a past-due user keeps Pro and is asked to update the card', async ({ browser }) => {
    const { page, close } = await openDashboard(browser, {
      subscriptions: [{ plan: 'pro', status: 'past_due', current_period_end: '2026-11-19T12:00:00' }],
      billingCustomer: { trial_used_at: null },
    });
    await expect(field(page, 'Current Plan')).toContainText('Pro');
    await expect(field(page, 'Status')).toContainText('Past due');
    await expect(card(page)).toContainText('payment failed');
    await close();
  });

  test('a free user with no billing account has no Manage billing button', async ({ browser }) => {
    const { page, close } = await openDashboard(browser, { subscriptions: [FREE], billingCustomer: null });
    await expect(field(page, 'Current Plan')).toContainText('Free');
    await expect(card(page).getByRole('button', { name: 'Start 14-day free trial' })).toBeVisible();
    await expect(card(page).getByRole('button', { name: 'Manage billing' })).toHaveCount(0);
    await close();
  });

  test('a free user who used the trial is offered Pro without one', async ({ browser }) => {
    const { page, close } = await openDashboard(browser, {
      subscriptions: [FREE],
      billingCustomer: { trial_used_at: '2026-09-01T00:00:00Z' },
    });
    await expect(card(page).getByRole('button', { name: 'Upgrade to Pro' })).toBeVisible();
    await expect(card(page).getByRole('button', { name: 'Start 14-day free trial' })).toHaveCount(0);
    await expect(card(page).getByRole('button', { name: 'Manage billing' })).toBeVisible();
    await close();
  });

  test('returning from checkout waits for the webhook and shows Pro', async ({ browser }) => {
    const { page, calls, close } = await openDashboard(
      browser,
      {
        // The first reads land before the webhook, the last after it.
        subscriptions: [
          FREE,
          FREE,
          { plan: 'pro', status: 'trialing', current_period_end: '2026-10-19T12:00:00' },
        ],
        billingCustomer: { trial_used_at: null },
      },
      '/dashboard?tab=subscription&session_id=cs_test_e2e',
    );
    await expect(card(page)).toContainText('Payment received');
    await expect(field(page, 'Current Plan')).toContainText('Pro', { timeout: 15_000 });
    await expect(card(page)).not.toContainText('Payment received');
    expect(calls.subscriptionReads).toBeGreaterThanOrEqual(3);
    await close();
  });

  test('checkout stays closed while payments are off', async ({ browser }) => {
    test.skip(checkoutLive, 'stripePayments is on and the live key is set');
    const { page, calls, close } = await openDashboard(browser, { subscriptions: [FREE], billingCustomer: null });
    const button = card(page).getByRole('button', { name: 'Start 14-day free trial' });
    await expect(button).toBeDisabled();
    await expect(card(page)).toContainText('Coming Soon');
    expect(calls.checkout).toEqual([]);
    await close();
  });

  test('checkout opens embedded with a client secret from billing-checkout', async ({ browser }) => {
    test.skip(!checkoutLive, 'stripePayments is off or the live publishable key is not set');
    const { page, calls, close } = await openDashboard(browser, { subscriptions: [FREE], billingCustomer: null });
    await card(page).getByRole('button', { name: 'Start 14-day free trial' }).click();
    await expect(page.getByTestId('stripe-embedded-checkout')).toContainText(
      `Embedded Checkout ${STRIPE_PUBLISHABLE_KEY_PROD} cs_test_e2e_secret`,
    );
    expect(calls.checkout).toEqual([{ plan: 'pro' }]);
    await close();
  });
});

import { expect, test, type Browser, type Page, type Route } from '@playwright/test';
import pkg from '../../package.json';
import { STRIPE_PUBLISHABLE_KEY_PROD } from '../src/lib/constants';

// The Team tab of /dashboard against the production build, reached from the
// home page through the navbar. Supabase (PostgREST and the organizations,
// billing-checkout and billing-portal functions) and Stripe.js are stubbed
// at the network layer. The stub keeps a small in-memory organization so
// each action shows up on the next read, the way the real tables would.
const checkoutLive =
  Boolean(pkg.featureFlags.stripePayments) && STRIPE_PUBLISHABLE_KEY_PROD !== '';

const ME = '6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10';
const MIA = '7a2d2e5f-7b9a-4b4a-8e66-3c2a1c4f0b21';
const ORG = '0b8d3f0e-2c1a-4f5e-9d7b-6a4c3e2f1a00';
const TOKEN = '0123456789abcdef'.repeat(4);
// Invite links carry the token in the fragment. Links in the older query
// form are still accepted when pasted.
const ACCEPT_URL = `https://treq.dev/dashboard?tab=team#invite=${TOKEN}`;
const OLD_ACCEPT_URL = `https://treq.dev/dashboard?tab=team&invite=${TOKEN}`;

const user = {
  id: ME,
  aud: 'authenticated',
  role: 'authenticated',
  email: 'owner-e2e@treq.dev',
  app_metadata: { provider: 'email' },
  user_metadata: { full_name: 'Olivia Owner' },
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

type Membership = { org_id: string; user_id: string; role: 'owner' | 'member'; created_at: string };
type Team = {
  organizations: { id: string; name: string }[];
  memberships: Membership[];
  profiles: { id: string; email: string; full_name: string | null }[];
  invites: { id: string; org_id: string; email: string; expires_at: string; created_at: string }[];
  subscriptions: {
    owner_id: string;
    status: string;
    current_period_end: string | null;
    cancel_at_period_end: boolean;
  }[];
  customers: { owner_id: string }[];
  installations: {
    id: number;
    account_login: string;
    account_type: string;
    organization_id: string | null;
    linked_user_id: string | null;
  }[];
  /** Answers the organizations function gives instead of succeeding. */
  refuse?: Record<string, { status: number; error: string; code: string }>;
};

const emptyTeam = (): Team => ({
  organizations: [],
  memberships: [],
  profiles: [{ id: ME, email: user.email, full_name: 'Olivia Owner' }],
  invites: [],
  subscriptions: [],
  customers: [],
  installations: [],
});

function acmeTeam(role: 'owner' | 'member' = 'owner'): Team {
  const team = emptyTeam();
  team.organizations = [{ id: ORG, name: 'Acme' }];
  team.memberships = [
    { org_id: ORG, user_id: ME, role, created_at: '2026-10-01T00:00:00Z' },
    {
      org_id: ORG,
      user_id: MIA,
      role: role === 'owner' ? 'member' : 'owner',
      created_at: '2026-10-02T00:00:00Z',
    },
  ];
  team.profiles.push({ id: MIA, email: 'mia@example.com', full_name: 'Mia Member' });
  if (role === 'owner') {
    team.invites = [
      {
        id: '9f9f9f9f-0000-4000-8000-000000000001',
        org_id: ORG,
        email: 'sam@example.com',
        expires_at: '2026-10-12T00:00:00Z',
        created_at: '2026-10-05T00:00:00Z',
      },
    ];
  }
  return team;
}

type Calls = { organizations: Record<string, unknown>[]; checkout: unknown[]; portal: unknown[] };

async function fulfillJson(route: Route, body: unknown, status = 200) {
  return route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

// Answers a PostgREST read: an object for `.single()`/`.maybeSingle()`, a list
// otherwise.
function rows(route: Route, list: unknown[]) {
  const wantsObject = (route.request().headers()['accept'] ?? '').includes('vnd.pgrst.object');
  return fulfillJson(route, wantsObject ? (list[0] ?? null) : list);
}

function handleOrganizations(team: Team, body: Record<string, unknown>) {
  const refusal = team.refuse?.[String(body.action)];
  if (refusal) return { status: refusal.status, body: { error: refusal.error, code: refusal.code } };
  switch (body.action) {
    case 'create': {
      team.organizations.push({ id: ORG, name: String(body.name) });
      team.memberships.push({ org_id: ORG, user_id: ME, role: 'owner', created_at: '2026-10-05T00:00:00Z' });
      return { status: 200, body: { organization: { id: ORG, name: body.name } } };
    }
    case 'invite': {
      const invite = {
        id: '9f9f9f9f-0000-4000-8000-000000000002',
        org_id: String(body.organization_id),
        email: String(body.email).toLowerCase(),
        expires_at: '2026-10-12T00:00:00Z',
        created_at: '2026-10-05T00:00:00Z',
      };
      team.invites.push(invite);
      return {
        status: 200,
        body: { invite: { id: invite.id, email: invite.email, expires_at: invite.expires_at }, accept_url: ACCEPT_URL },
      };
    }
    case 'revoke_invite':
      team.invites = team.invites.filter((i) => i.id !== body.invite_id);
      return { status: 200, body: { ok: true } };
    case 'accept':
      team.organizations.push({ id: ORG, name: 'Acme' });
      team.memberships.push({ org_id: ORG, user_id: ME, role: 'member', created_at: '2026-10-05T00:00:00Z' });
      return { status: 200, body: { organization: { id: ORG, name: 'Acme' }, result: 'joined' } };
    case 'leave':
      team.memberships = team.memberships.filter((m) => m.user_id !== ME);
      team.organizations = [];
      return { status: 200, body: { ok: true } };
    case 'remove_member':
      team.memberships = team.memberships.filter((m) => m.user_id !== body.user_id);
      return { status: 200, body: { ok: true } };
    case 'promote_member':
    case 'demote_owner': {
      const role = body.action === 'promote_member' ? 'owner' : 'member';
      for (const m of team.memberships) {
        if (m.org_id === body.organization_id && m.user_id === body.user_id) m.role = role;
      }
      return {
        status: 200,
        body: { ok: true, result: role === 'owner' ? 'promoted' : 'demoted' },
      };
    }
    case 'delete_organization':
      team.memberships = team.memberships.filter((m) => m.org_id !== body.organization_id);
      team.organizations = team.organizations.filter((o) => o.id !== body.organization_id);
      return { status: 200, body: { ok: true } };
    case 'attach_installation':
      for (const i of team.installations) {
        if (i.id === body.installation_id) i.organization_id = String(body.organization_id);
      }
      return { status: 200, body: { ok: true, result: 'attached' } };
    default:
      return { status: 400, body: { error: 'unknown action' } };
  }
}

async function openTeamTab(
  browser: Browser,
  team: Team,
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
          localStorage: [{ name: `sb-${ref}-auth-token`, value: JSON.stringify(session) }],
        },
      ],
    },
  });
  const page = await context.newPage();
  const calls: Calls = { organizations: [], checkout: [], portal: [] };
  const mine = () => new Set(team.memberships.filter((m) => m.user_id === ME).map((m) => m.org_id));

  await page.route(/\/auth\/v1\//, (route) =>
    fulfillJson(route, route.request().url().includes('/token') ? session : user),
  );
  await page.route(/\/rest\/v1\/subscriptions(\?|$)/, (route) =>
    rows(route, [{ plan: 'free', status: 'inactive', current_period_end: null }]),
  );
  await page.route(/\/rest\/v1\/organization_members(\?|$)/, (route) =>
    rows(
      route,
      team.memberships
        .filter((m) => mine().has(m.org_id))
        .map((m) => ({ ...m, organizations: team.organizations.find((o) => o.id === m.org_id) })),
    ),
  );
  await page.route(/\/rest\/v1\/profiles(\?|$)/, (route) => rows(route, team.profiles));
  await page.route(/\/rest\/v1\/organization_invites(\?|$)/, (route) => rows(route, team.invites));
  await page.route(/\/rest\/v1\/billing_subscriptions(\?|$)/, (route) => rows(route, team.subscriptions));
  await page.route(/\/rest\/v1\/billing_customers(\?|$)/, (route) =>
    rows(route, route.request().url().includes('organization') ? team.customers : []),
  );
  await page.route(/\/rest\/v1\/github_app_installations(\?|$)/, (route) =>
    rows(route, team.installations),
  );
  await page.route(/\/rest\/v1\/github_repositories(\?|$)/, (route) => rows(route, []));
  await page.route(/\/functions\/v1\/organizations/, async (route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204 });
    const body = route.request().postDataJSON() as Record<string, unknown>;
    calls.organizations.push(body);
    const answer = handleOrganizations(team, body);
    return fulfillJson(route, answer.body, answer.status);
  });
  await page.route(/\/functions\/v1\/billing-checkout/, async (route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204 });
    calls.checkout.push(route.request().postDataJSON());
    return fulfillJson(route, { client_secret: 'cs_test_team_secret' });
  });
  await page.route(/\/functions\/v1\/billing-portal/, async (route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204 });
    calls.portal.push(route.request().postDataJSON());
    return fulfillJson(route, { url: 'https://billing.stripe.com/p/session/team-e2e' });
  });
  await page.route('https://billing.stripe.com/**', (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: '<h1>Stripe customer portal</h1>' }),
  );
  await page.route('https://js.stripe.com/**', (route) =>
    route.fulfill({ status: 200, contentType: 'application/javascript', body: FAKE_STRIPE_JS }),
  );

  await page.goto('/');
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Dashboard' }).click();
  await page.getByRole('button', { name: 'Team', exact: true }).click();
  return { page, calls, close: () => context.close() };
}

const teamSection = (page: Page) => page.getByRole('region', { name: 'Team' });
const orgCard = (page: Page, name: string) => teamSection(page).getByRole('region', { name });

test.describe('Dashboard team tab', () => {
  test('an owner sees seats, members and pending invites, and invites with a shareable link', async ({
    browser,
  }) => {
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('owner'));
    const acme = orgCard(page, 'Acme');

    await expect(acme.getByRole('group', { name: 'Your role' })).toContainText('Owner');
    await expect(acme.getByRole('group', { name: 'Plan' })).toContainText('No Team subscription');
    await expect(acme.getByRole('group', { name: 'Seats' })).toContainText('2 of 5 included');
    await expect(acme.getByRole('list', { name: 'Members' })).toContainText('Mia Member');
    await expect(acme.getByRole('list', { name: 'Pending invites' })).toContainText('sam@example.com');

    await acme.getByRole('textbox', { name: 'Email to invite' }).fill('lee@example.com');
    await acme.getByRole('button', { name: 'Invite', exact: true }).click();
    await expect(acme.getByRole('textbox', { name: 'Invite link for lee@example.com' })).toHaveValue(
      ACCEPT_URL,
    );
    expect(calls.organizations).toContainEqual({
      action: 'invite',
      organization_id: ORG,
      email: 'lee@example.com',
    });

    await acme.getByRole('button', { name: 'Revoke invite for sam@example.com' }).click();
    await expect(acme.getByRole('list', { name: 'Pending invites' })).not.toContainText('sam@example.com');
    expect(calls.organizations).toContainEqual({
      action: 'revoke_invite',
      invite_id: '9f9f9f9f-0000-4000-8000-000000000001',
    });
    await close();
  });

  test('a refused invite shows the message the server names', async ({ browser }) => {
    const team = acmeTeam('owner');
    team.refuse = {
      invite: {
        status: 409,
        error: 'That address already belongs to a member.',
        code: 'already_member',
      },
    };
    const { page, close } = await openTeamTab(browser, team);
    const acme = orgCard(page, 'Acme');
    await acme.getByRole('textbox', { name: 'Email to invite' }).fill('mia@example.com');
    await acme.getByRole('button', { name: 'Invite', exact: true }).click();
    await expect(acme.getByRole('alert')).toContainText('That address already belongs to a member.');
    await close();
  });

  test('an owner removes a member', async ({ browser }) => {
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('owner'));
    const acme = orgCard(page, 'Acme');
    page.once('dialog', (dialog) => dialog.accept());
    await acme.getByRole('button', { name: 'Remove Mia Member' }).click();
    await expect(acme.getByRole('list', { name: 'Members' })).not.toContainText('Mia Member');
    expect(calls.organizations).toContainEqual({
      action: 'remove_member',
      organization_id: ORG,
      user_id: MIA,
    });
    await close();
  });

  test('an owner makes a member an owner, and back', async ({ browser }) => {
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('owner'));
    const acme = orgCard(page, 'Acme');
    const mia = acme.getByRole('list', { name: 'Members' }).getByRole('listitem').filter({ hasText: 'Mia Member' });
    await expect(acme).toContainText('You are the only owner. To transfer ownership, make another member an owner, then leave.');

    page.once('dialog', (dialog) => dialog.accept());
    await acme.getByRole('button', { name: 'Make Mia Member an owner' }).click();
    await expect(mia).toContainText('Owner');
    await expect(acme).toContainText('To transfer ownership, make another member an owner, then leave.');
    await expect(acme).not.toContainText('You are the only owner');
    expect(calls.organizations).toContainEqual({
      action: 'promote_member',
      organization_id: ORG,
      user_id: MIA,
    });

    page.once('dialog', (dialog) => dialog.accept());
    await acme.getByRole('button', { name: 'Make Mia Member a member' }).click();
    await expect(acme.getByRole('button', { name: 'Make Mia Member an owner' })).toBeVisible();
    expect(calls.organizations).toContainEqual({
      action: 'demote_owner',
      organization_id: ORG,
      user_id: MIA,
    });
    await close();
  });

  test('the last owner is told why they cannot step down', async ({ browser }) => {
    const team = acmeTeam('owner');
    team.refuse = {
      leave: {
        status: 409,
        error: 'An organization needs at least one owner.',
        code: 'last_owner',
      },
    };
    const { page, close } = await openTeamTab(browser, team);
    const acme = orgCard(page, 'Acme');
    page.once('dialog', (dialog) => dialog.accept());
    await acme.getByRole('button', { name: 'Leave Acme' }).click();
    await expect(acme.getByRole('alert')).toContainText('An organization needs at least one owner.');
    await close();
  });

  test('an owner deletes the organization', async ({ browser }) => {
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('owner'));
    page.once('dialog', (dialog) => dialog.accept());
    await orgCard(page, 'Acme').getByRole('button', { name: 'Delete Acme' }).click();
    await expect(orgCard(page, 'Acme')).toHaveCount(0);
    expect(calls.organizations).toContainEqual({ action: 'delete_organization', organization_id: ORG });
    await close();
  });

  test('deleting an organization that has Team says to cancel it first', async ({ browser }) => {
    const team = acmeTeam('owner');
    team.subscriptions = [
      { owner_id: ORG, status: 'active', current_period_end: '2026-11-05T00:00:00', cancel_at_period_end: false },
    ];
    team.refuse = {
      delete_organization: {
        status: 409,
        error:
          'This organization has a Team subscription. Cancel it with Manage billing, then delete the organization once the subscription has ended.',
        code: 'team_subscription_active',
      },
    };
    const { page, close } = await openTeamTab(browser, team);
    const acme = orgCard(page, 'Acme');
    page.once('dialog', (dialog) => dialog.accept());
    await acme.getByRole('button', { name: 'Delete Acme' }).click();
    await expect(acme.getByRole('alert')).toContainText('Cancel it with Manage billing');
    await expect(acme.getByRole('group', { name: 'Plan' })).toContainText('Team');
    await close();
  });

  test('buying Team stays closed while payments are off', async ({ browser }) => {
    test.skip(checkoutLive, 'stripePayments is on and the live key is set');
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('owner'));
    const acme = orgCard(page, 'Acme');
    await expect(acme.getByRole('button', { name: 'Buy Team' })).toBeDisabled();
    await expect(acme).toContainText('Coming Soon');
    expect(calls.checkout).toEqual([]);
    await close();
  });

  test('buying Team opens embedded checkout for the organization', async ({ browser }) => {
    test.skip(!checkoutLive, 'stripePayments is off or the live publishable key is not set');
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('owner'));
    const acme = orgCard(page, 'Acme');
    await acme.getByRole('button', { name: 'Buy Team' }).click();
    await expect(acme.getByTestId('stripe-embedded-checkout')).toContainText('cs_test_team_secret');
    expect(calls.checkout).toEqual([{ plan: 'team', organization_id: ORG }]);
    await close();
  });

  test('an owner with Team sees its status and manages billing', async ({ browser }) => {
    const team = acmeTeam('owner');
    team.subscriptions = [
      { owner_id: ORG, status: 'active', current_period_end: '2026-11-05T00:00:00', cancel_at_period_end: false },
    ];
    team.customers = [{ owner_id: ORG }];
    const { page, calls, close } = await openTeamTab(browser, team);
    const acme = orgCard(page, 'Acme');
    await expect(acme.getByRole('group', { name: 'Plan' })).toContainText('Team');
    await expect(acme.getByRole('group', { name: 'Plan' })).toContainText('Active');
    await expect(acme.getByRole('button', { name: 'Buy Team' })).toHaveCount(0);
    await acme.getByRole('button', { name: 'Manage billing' }).click();
    await expect(page.getByRole('heading', { name: 'Stripe customer portal' })).toBeVisible();
    expect(calls.portal).toEqual([{ organization_id: ORG }]);
    await close();
  });

  test('an owner attaches a GitHub App installation they linked', async ({ browser }) => {
    const team = acmeTeam('owner');
    team.installations = [
      { id: 4242, account_login: 'acme-gh', account_type: 'Organization', organization_id: null, linked_user_id: ME },
    ];
    const { page, calls, close } = await openTeamTab(browser, team);
    const acme = orgCard(page, 'Acme');
    await acme.getByRole('button', { name: 'Attach acme-gh' }).click();
    await expect(acme.getByRole('list', { name: 'GitHub App installations' })).toContainText('acme-gh');
    expect(calls.organizations).toContainEqual({
      action: 'attach_installation',
      organization_id: ORG,
      installation_id: 4242,
    });
    await close();
  });

  test('a member sees the team but cannot manage it, and can leave', async ({ browser }) => {
    const { page, calls, close } = await openTeamTab(browser, acmeTeam('member'));
    const acme = orgCard(page, 'Acme');
    await expect(acme.getByRole('group', { name: 'Your role' })).toContainText('Member');
    await expect(acme.getByRole('list', { name: 'Members' })).toContainText('Mia Member');
    await expect(acme.getByRole('textbox', { name: 'Email to invite' })).toHaveCount(0);
    await expect(acme.getByRole('button', { name: /^Remove / })).toHaveCount(0);
    await expect(acme.getByRole('button', { name: /^Make / })).toHaveCount(0);
    await expect(acme.getByRole('button', { name: 'Delete Acme' })).toHaveCount(0);
    await expect(acme.getByRole('button', { name: 'Buy Team' })).toHaveCount(0);

    page.once('dialog', (dialog) => dialog.accept());
    await acme.getByRole('button', { name: 'Leave Acme' }).click();
    await expect(orgCard(page, 'Acme')).toHaveCount(0);
    expect(calls.organizations).toContainEqual({ action: 'leave', organization_id: ORG });
    await close();
  });

  test('a new user creates an organization', async ({ browser }) => {
    const { page, calls, close } = await openTeamTab(browser, emptyTeam());
    await teamSection(page).getByRole('textbox', { name: 'Organization name' }).fill('Acme');
    await teamSection(page).getByRole('button', { name: 'Create organization' }).click();
    await expect(orgCard(page, 'Acme').getByRole('group', { name: 'Your role' })).toContainText('Owner');
    expect(calls.organizations).toContainEqual({ action: 'create', name: 'Acme' });
    await close();
  });

  for (const [form, link] of [
    ['fragment', ACCEPT_URL],
    ['older query', OLD_ACCEPT_URL],
  ]) {
    test(`an invited user joins with the link they were sent (${form} form)`, async ({ browser }) => {
      const { page, calls, close } = await openTeamTab(browser, emptyTeam());
      await teamSection(page).getByRole('textbox', { name: 'Invite link', exact: true }).fill(link);
      await teamSection(page).getByRole('button', { name: 'Join' }).click();
      await expect(teamSection(page).getByRole('status')).toContainText('You joined Acme');
      await expect(orgCard(page, 'Acme').getByRole('group', { name: 'Your role' })).toContainText('Member');
      expect(calls.organizations).toContainEqual({ action: 'accept', token: TOKEN });
      await close();
    });
  }
});

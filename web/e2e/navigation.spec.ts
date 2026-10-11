import { test, expect, type Page } from '@playwright/test';
import pkg from '../../package.json';

const nav = (page: Page) => page.getByRole('navigation', { name: 'Main' });
const sidebar = (page: Page) => page.getByRole('navigation', { name: 'Docs sidebar' });
const footer = (page: Page) => page.getByRole('contentinfo');

async function clickNavDropdownItem(
  page: Page,
  dropdown: 'Discover' | 'Product',
  item: string,
) {
  const main = nav(page);
  await main.getByRole('button', { name: dropdown }).hover();
  await main.getByRole('link', { name: item, exact: true }).click();
}

test.describe('Homepage Features layout', () => {
  test('review screenshot keeps aspect ratio and takes most of the row', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto('/');
    const features = page.getByRole('region', { name: 'Features' });
    await expect(features.getByRole('heading', { name: 'Features', level: 2 })).toBeVisible();
    await page.waitForLoadState('networkidle');
    await features.scrollIntoViewIfNeeded();

    const screenshot = features.getByRole('img', {
      name: 'Treq code review screenshot showing comments sent to Claude',
    });
    await expect(screenshot).toBeVisible();

    const box = await screenshot.boundingBox();
    expect(box).not.toBeNull();

    // Intrinsic size comes from the HTML width/height attrs on the <img>.
    const naturalWidth = Number(await screenshot.getAttribute('width'));
    const naturalHeight = Number(await screenshot.getAttribute('height'));
    expect(naturalWidth).toBeGreaterThan(0);
    expect(naturalHeight).toBeGreaterThan(0);

    // Regression: HTML width/height attrs without CSS height:auto left the
    // image at intrinsic height (1749px) while width shrank, stretching it.
    const renderedAspect = box!.width / box!.height;
    const naturalAspect = naturalWidth / naturalHeight;
    expect(Math.abs(renderedAspect - naturalAspect)).toBeLessThan(0.05);
    expect(box!.height).toBeLessThan(naturalHeight * 0.5);

    // Screenshot is 2fr of a 1fr/2fr row inside the Features section (~2/3 width).
    const featuresBox = await features.boundingBox();
    expect(featuresBox).not.toBeNull();
    expect(box!.width / featuresBox!.width).toBeGreaterThanOrEqual(0.55);
  });
});

test.describe('Navbar navigation', () => {
  test('shows desktop platform availability', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByLabel('macOS')).toBeVisible();
    await expect(page.getByLabel('Windows')).toBeVisible();
    await expect(page.getByLabel('Linux')).toBeVisible();
  });

  test('Learn link navigates to learn section', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await expect(page.getByRole('heading', { name: 'Learn', level: 1 })).toBeVisible();
  });

  test('Documentation link navigates to docs section', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await expect(page.getByRole('heading', { name: 'Treq', level: 1 })).toBeVisible();
  });

  test('Download button links to the latest release when signed out', async ({ page }) => {
    await page.goto('/');
    await expect(nav(page).getByRole('link', { name: 'Download', exact: true }))
      .toHaveAttribute('href', /^https:\/\/github\.com\/tznc\/treq\/releases\//);
  });

  test('installation lists each release target', async ({ page }) => {
    await page.goto('/');
    await footer(page).getByRole('link', { name: 'Installation' }).click();
    await expect(page.getByRole('heading', { name: 'Installation and Quickstart', level: 1 })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'x86_64-pc-windows-msvc' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'x86_64-unknown-linux-gnu' })).toBeVisible();
  });

  test('logo navigates home from a docs page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await nav(page).getByRole('link', { name: 'Treq Logo' }).click();
    await expect(page.getByRole('heading', { name: /Stacking Agent Development Environment/, level: 1 })).toBeVisible();
  });

  test.describe('on mobile', () => {
    test.use({ viewport: { width: 375, height: 667 } });

    test('hides search, Sign in, and GitHub from the top bar', async ({ page }) => {
      await page.goto('/');
      await expect(page.getByPlaceholder('Search...')).not.toBeVisible();
      await expect(page.getByRole('link', { name: 'Sign in' })).not.toBeVisible();
      await expect(page.getByRole('link', { name: 'GitHub repository' })).not.toBeVisible();
    });

    test('shows search, Sign in, and GitHub in the sidebar drawer', async ({ page }) => {
      await page.goto('/');
      await page.getByLabel('Toggle navigation bar').click();
      await expect(page.getByPlaceholder('Search...').filter({ visible: true })).toBeVisible();
      await expect(page.getByRole('link', { name: 'Sign in' }).filter({ visible: true })).toBeVisible();
      await expect(
        page.getByRole('link', { name: 'GitHub repository' }).filter({ visible: true }),
      ).toBeVisible();
    });
  });

  test('GitHub link points to the correct repo', async ({ page }) => {
    await page.goto('/');
    await expect(nav(page).getByRole('link', { name: 'GitHub repository' }))
      .toHaveAttribute('href', 'https://github.com/tznc/treq');
  });

  test('Sign in link navigates to login page when signed out', async ({ page }) => {
    await page.goto('/');
    await expect(nav(page).getByRole('link', { name: 'Sign in' }))
      .toHaveAttribute('href', '/sign-in');
  });

  test('email sign in is hidden when email signup is disabled', async ({ page }) => {
    await page.goto('/');
    await nav(page).getByRole('link', { name: 'Sign in' }).click();

    await expect(page.getByRole('button', { name: 'Continue with Google' })).toBeVisible();
    await expect(page.getByPlaceholder('Email address')).toHaveCount(0);
    await expect(page.getByPlaceholder('Password')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Sign In', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Forgot password?' })).toHaveCount(0);
  });

  test('signed-in navbar shows Dashboard and Open App', async ({ browser }) => {
    const now = Math.floor(Date.now() / 1000);
    const user = {
      id: 'e2e-user-id',
      aud: 'authenticated',
      role: 'authenticated',
      email: 'e2e@treq.dev',
      app_metadata: { provider: 'email' },
      user_metadata: {},
      created_at: new Date().toISOString(),
    };
    const session = {
      access_token: [
        Buffer.from(JSON.stringify({ alg: 'none', typ: 'JWT' })).toString('base64url'),
        Buffer.from(JSON.stringify({ sub: user.id, email: user.email, role: 'authenticated', exp: now + 3600 })).toString('base64url'),
        'e2e',
      ].join('.'),
      refresh_token: 'e2e-refresh-token',
      expires_in: 3600,
      expires_at: now + 3600,
      token_type: 'bearer',
      user,
    };
    const supabaseProjectRef = new URL(pkg.env.prod.supabase.url).hostname.split('.')[0];

    // Email signup is disabled, so seed an existing session via storageState
    // instead of driving the password form (or using addInitScript).
    const context = await browser.newContext({
      baseURL: 'http://localhost:3000',
      storageState: {
        cookies: [],
        origins: [
          {
            origin: 'http://localhost:3000',
            localStorage: [
              {
                // The E2E server serves a production build, so Supabase reads
                // the storage key derived from the production project URL.
                name: `sb-${supabaseProjectRef}-auth-token`,
                value: JSON.stringify(session),
              },
            ],
          },
        ],
      },
    });
    const page = await context.newPage();

    await page.route(/\/auth\/v1\//, async (route) => {
      const url = route.request().url();
      if (url.includes('/user')) {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(user),
        });
        return;
      }
      if (url.includes('/token')) {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(session),
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: '{}',
      });
    });

    await page.goto('/');

    await expect(nav(page).getByRole('link', { name: 'Dashboard' })).toHaveAttribute(
      'href',
      '/dashboard',
    );
    await expect(nav(page).getByRole('link', { name: 'Open App' })).toHaveAttribute(
      'href',
      'treq://',
    );
    await expect(nav(page).getByRole('link', { name: 'Sign in' })).toHaveCount(0);
    await expect(nav(page).getByRole('link', { name: 'Download', exact: true })).toHaveCount(0);

    await context.close();
  });

  test('Pricing link navigates to pricing page', async ({ page }) => {
    await page.goto('/');
    await nav(page).getByRole('link', { name: 'Pricing' }).click();
    await expect(page.getByRole('heading', { name: 'Pricing', level: 1 })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Free', level: 2 })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Pro', level: 2 })).toBeVisible();
  });

  test('Roadmap link navigates to roadmap page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Roadmap');
    await expect(page.getByRole('heading', { name: 'Roadmap', level: 1 })).toBeVisible();
  });

  test('Tools link navigates to tools page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Tools');
    await expect(page.getByRole('heading', { name: 'Tools', level: 1 })).toBeVisible();
  });

  test('Changelog link navigates to changelog page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Changelog');
    await expect(page.getByRole('heading', { name: 'Changelog', level: 1 })).toBeVisible();
  });
});

test.describe('Docs sidebar navigation', () => {
  test('Installation link is visible by default on Docs overview', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await expect(sidebar(page).getByRole('link', { name: 'Installation' })).toBeVisible();
  });

  test('Installation link navigates to installation page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Installation' }).click();
    await expect(page.getByRole('heading', { name: 'Installation and Quickstart', level: 1 })).toBeVisible();
  });

  test('Concepts link navigates to concepts overview', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Concepts' }).click();
    await expect(page.getByRole('heading', { name: 'Concepts', level: 1 })).toBeVisible();
  });

  test('Workspaces link is visible after navigating into Concepts', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Concepts' }).click();
    await expect(sidebar(page).getByRole('link', { name: 'Workspaces' })).toBeVisible();
  });

  test('Workspaces link navigates to workspaces page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Concepts' }).click();
    await sidebar(page).getByRole('link', { name: 'Workspaces' }).click();
    await expect(page.getByRole('heading', { name: 'Workspaces', level: 1 })).toBeVisible();
  });

  test('Reference expands to show CLI link', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('button', { name: 'Reference' }).click();
    await expect(sidebar(page).getByRole('link', { name: 'CLI' })).toBeVisible();
  });

  test('CLI link navigates to CLI page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('button', { name: 'Reference' }).click();
    await sidebar(page).getByRole('link', { name: 'CLI' }).click();
    await expect(page.getByRole('heading', { name: 'CLI', level: 1 })).toBeVisible();
  });

  test('Keyboard Shortcuts link navigates to keyboard shortcuts page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('button', { name: 'Reference' }).click();
    await sidebar(page).getByRole('link', { name: 'Keyboard Shortcuts' }).click();
    await expect(page.getByRole('heading', { name: 'Keyboard Shortcuts', level: 1 })).toBeVisible();
  });

  test('Tutorials link navigates to tutorials overview', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Tutorials' }).click();
    await expect(page.getByRole('heading', { name: 'Tutorials', level: 1 })).toBeVisible();
  });

  test('Committing Changes link navigates to committing changes page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Tutorials' }).click();
    await sidebar(page).getByRole('link', { name: 'Committing Changes' }).click();
    await expect(page.getByRole('heading', { name: 'Committing Changes', level: 1 })).toBeVisible();
  });
});

test.describe('Learn sidebar navigation', () => {
  test('Tutorials link navigates to tutorials overview', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await sidebar(page).getByRole('link', { name: 'Tutorials' }).click();
    await expect(page.getByRole('heading', { name: 'Tutorials', level: 1 })).toBeVisible();
  });

  test('Setting Up Claude Code link navigates to Claude Code tutorial', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await sidebar(page).getByRole('link', { name: 'Tutorials' }).click();
    await page.locator('article').getByRole('link', { name: 'Setting Up Claude Code' }).click();
    await expect(page.getByRole('heading', { name: 'Setting Up Claude Code', level: 1 })).toBeVisible();
  });

  test('How-To link navigates to how-to overview', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await sidebar(page).getByRole('link', { name: 'How-To' }).click();
    await expect(page.getByRole('heading', { name: 'How-To Guides', level: 1 })).toBeVisible();
  });

  test('Docs Pushing to Remote link navigates to pushing to remote page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'How-To' }).click();
    await page.locator('article').getByRole('link', { name: 'Pushing to Remote' }).click();
    await expect(page.getByRole('heading', { name: 'Pushing to Remote', level: 1 })).toBeVisible();
  });
});

test.describe('Cross-section navigation', () => {
  test('switches from Learn to Docs via navbar', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await expect(page.getByRole('heading', { name: 'Treq', level: 1 })).toBeVisible();
  });

  test('switches from Docs to Learn via navbar', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await clickNavDropdownItem(page, 'Discover', 'Learn');
    await expect(page.getByRole('heading', { name: 'Learn', level: 1 })).toBeVisible();
  });

  test('logo returns home from an inner page', async ({ page }) => {
    await page.goto('/');
    await clickNavDropdownItem(page, 'Product', 'Documentation');
    await sidebar(page).getByRole('link', { name: 'Installation' }).click();
    await nav(page).getByRole('link', { name: 'Treq Logo' }).click();
    await expect(page.getByRole('heading', { name: /Stacking Agent Development Environment/, level: 1 })).toBeVisible();
  });
});

test.describe('Footer navigation', () => {
  test('Installation link navigates to installation page', async ({ page }) => {
    await page.goto('/');
    await footer(page).getByRole('link', { name: 'Installation' }).click();
    await expect(page.getByRole('heading', { name: 'Installation and Quickstart', level: 1 })).toBeVisible();
  });

  test('Learn link navigates to learn section', async ({ page }) => {
    await page.goto('/');
    await footer(page).getByRole('link', { name: 'Learn' }).click();
    await expect(page.getByRole('heading', { name: 'Learn', level: 1 })).toBeVisible();
  });

  test('Security and Privacy link navigates to security page', async ({ page }) => {
    await page.goto('/');
    await footer(page).getByRole('link', { name: 'Security and Privacy' }).click();
    await expect(page.getByRole('heading', { name: 'Security and Privacy', level: 1 })).toBeVisible();
  });

  test('GitHub link points to the correct repo', async ({ page }) => {
    await page.goto('/');
    await expect(footer(page).getByRole('link', { name: 'GitHub' }))
      .toHaveAttribute('href', 'https://github.com/tznc/treq');
  });

  test('Pricing link navigates to pricing page', async ({ page }) => {
    await page.goto('/');
    await footer(page).getByRole('link', { name: 'Pricing' }).click();
    await expect(page.getByRole('heading', { name: 'Pricing', level: 1 })).toBeVisible();
  });

  test('Roadmap link navigates to roadmap page', async ({ page }) => {
    await page.goto('/');
    await footer(page).getByRole('link', { name: 'Roadmap' }).click();
    await expect(page.getByRole('heading', { name: 'Roadmap', level: 1 })).toBeVisible();
  });
});

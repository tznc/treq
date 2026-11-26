import { test, expect, type Page } from '@playwright/test';

const UA = {
  mac: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  windows: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  linux: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  android: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36',
  iphone: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
};

// Asset URLs come from the live releases API, which reports the repo's
// canonical owner rather than an alias, so the owner is not pinned.
const RELEASE_ASSET = /^https:\/\/github\.com\/[^/]+\/treq\/releases\/download\/v[\d.]+\//;

// The navbar's Download button is the way in from the landing page.
async function openDownloadPage(page: Page) {
  await page.goto('/');
  await page
    .getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Download', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: 'Download Treq', level: 1 })).toBeVisible();
}

test.describe('Download page', () => {
  for (const [ua, name, others] of [
    ['mac', 'macOS', ['Windows', 'Linux']],
    ['windows', 'Windows', ['macOS', 'Linux']],
    ['linux', 'Linux', ['macOS', 'Windows']],
  ] as const) {
    test.describe(`on ${name}`, () => {
      test.use({ userAgent: UA[ua] });

      test(`focuses ${name} and lists the other platforms`, async ({ page }) => {
        await openDownloadPage(page);
        const focus = page.getByRole('region', { name: `Download for ${name}` });
        await expect(focus.getByText('Your system')).toBeVisible();
        await expect(focus.getByRole('link').first()).toHaveAttribute('href', RELEASE_ASSET);
        const list = page.getByRole('region', { name: 'Other platforms' });
        for (const other of others) {
          await expect(list.getByRole('heading', { name: other, level: 3 })).toBeVisible();
        }
        await expect(list.getByRole('heading', { name, level: 3 })).toHaveCount(0);
      });
    });
  }

  test.describe('on an iPhone', () => {
    test.use({ userAgent: UA.iphone });

    test('shows macOS without claiming it is the visitor’s system', async ({ page }) => {
      await openDownloadPage(page);
      const focus = page.getByRole('region', { name: 'Download for macOS' });
      await expect(focus.getByRole('link').first()).toHaveAttribute('href', RELEASE_ASSET);
      await expect(focus.getByText('Your system')).toHaveCount(0);
    });
  });
});

test.describe('Landing page download links on macOS', () => {
  test.use({ userAgent: UA.mac });

  test('open the download page instead of an installer', async ({ page }) => {
    await page.goto('/');
    const button = page.getByRole('link', { name: 'Download for macOS' }).first();
    // The OS label appears after hydration, when installer links used to swap in.
    await expect(button).toBeVisible();
    await expect(page.locator('a[href*="/releases/download/"]')).toHaveCount(0);
    await button.click();
    await expect(page).toHaveURL(/\/download\/$/);
    await expect(page.getByRole('region', { name: 'Download for macOS' })).toBeVisible();
  });
});

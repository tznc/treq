import { test, expect, type Page } from '@playwright/test';

const QUERY = 'cli workspace status';
const QUERY_ENCODED = encodeURIComponent(QUERY);

// Known result for QUERY: the CLI reference page whose excerpt contains "workspace status"
const EXPECTED_RESULT_URL = '/docs/reference/cli/';
const EXPECTED_RESULT_TITLE = 'CLI';
const EXPECTED_RESULT_KEYWORD = 'status';

const nav = (page: Page) => page.getByRole('navigation', { name: 'Main' });
const searchInput = (page: Page) => nav(page).getByPlaceholder('Search...');
const searchDropdown = (page: Page) => page.getByRole('link', { name: /see all results/i }).locator('..');

async function navigateToSearchPage(page: Page, query: string) {
  await page.goto('/');
  await searchInput(page).fill(query);
  await searchInput(page).press('Enter');
}

test.describe('Search bar (navbar)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('search input is visible in the navbar', async ({ page }) => {
    await expect(searchInput(page)).toBeVisible();
  });

  test('typing a query shows results dropdown', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    await expect(searchDropdown(page).locator('a').first()).toBeVisible();
  });

  test('results include the CLI page with a status-related excerpt', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    const cliResult = searchDropdown(page).locator(`a[href="${EXPECTED_RESULT_URL}"]`);
    await expect(cliResult).toBeVisible();
    await expect(cliResult.getByText(EXPECTED_RESULT_TITLE).first()).toBeVisible();
    const excerptText = await cliResult.textContent();
    expect(excerptText?.toLowerCase()).toContain(EXPECTED_RESULT_KEYWORD);
  });

  test('dropdown shows "See all results" link', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    await expect(searchDropdown(page).getByRole('link', { name: /see all results/i })).toBeVisible();
  });

  test('clicking the CLI result navigates to the CLI reference page', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    await searchDropdown(page).locator(`a[href="${EXPECTED_RESULT_URL}"]`).click();
    await expect(page.getByRole('heading', { name: EXPECTED_RESULT_TITLE, level: 1 })).toBeVisible();
  });

  test('pressing Enter navigates to /search with results', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await searchInput(page).press('Enter');
    await expect(page.getByText(/no results found/i)).not.toBeVisible({ timeout: 10_000 });
  });

  test('dropdown closes on Escape', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    await page.keyboard.press('Escape');
    await expect(searchDropdown(page)).not.toBeVisible();
  });

  test('dropdown closes when clicking outside', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    await page.mouse.click(10, 10);
    await expect(searchDropdown(page)).not.toBeVisible();
  });

  test('clearing the input hides results', async ({ page }) => {
    await searchInput(page).fill(QUERY);
    await expect(searchDropdown(page)).toBeVisible({ timeout: 10_000 });
    await searchInput(page).fill('');
    await expect(searchDropdown(page)).not.toBeVisible();
  });

  test('unmatched query shows no dropdown', async ({ page }) => {
    await searchInput(page).fill('zzzznotarealterm');
    await page.waitForTimeout(1000);
    await expect(searchDropdown(page)).not.toBeVisible();
  });
});

test.describe('Search results page (/search)', () => {
  test('shows results for a matching query', async ({ page }) => {
    await navigateToSearchPage(page, QUERY);
    await expect(page.getByRole('heading', { name: /search results for/i })).toBeVisible();
    await expect(page.getByRole('link', { name: EXPECTED_RESULT_TITLE, exact: true }))
      .toBeVisible({ timeout: 10_000 });
  });

  test('CLI page result appears with a status-related excerpt', async ({ page }) => {
    await navigateToSearchPage(page, QUERY);
    const cliResult = page.locator(`ul li:has(a[href="${EXPECTED_RESULT_URL}"])`);
    await expect(cliResult).toBeVisible({ timeout: 10_000 });
    const excerptText = await cliResult.textContent();
    expect(excerptText?.toLowerCase()).toContain(EXPECTED_RESULT_KEYWORD);
  });

  test('clicking CLI result navigates to the CLI reference page', async ({ page }) => {
    await navigateToSearchPage(page, QUERY);
    const link = page.locator(`ul li a[href="${EXPECTED_RESULT_URL}"]`);
    await expect(link).toBeVisible({ timeout: 10_000 });
    await link.click();
    await expect(page.getByRole('heading', { name: EXPECTED_RESULT_TITLE, level: 1 })).toBeVisible();
  });

  test('shows no-results message for unmatched query', async ({ page }) => {
    await navigateToSearchPage(page, 'zzzznotarealterm');
    await expect(page.getByText(/no results found/i)).toBeVisible({ timeout: 10_000 });
  });

  test('result excerpts contain highlighted terms', async ({ page }) => {
    await navigateToSearchPage(page, QUERY);
    const cliResult = page.locator(`ul li:has(a[href="${EXPECTED_RESULT_URL}"])`);
    await expect(cliResult).toBeVisible({ timeout: 10_000 });
    await expect(cliResult.locator('mark').first()).toBeVisible();
  });
});

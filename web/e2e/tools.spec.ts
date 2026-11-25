import { test, expect } from '@playwright/test';

async function navigateToTools(page: import('@playwright/test').Page) {
  await page.goto('/tools/');
}

async function navigateToTool(
  page: import('@playwright/test').Page,
  name: 'Branch Visualizer',
) {
  await navigateToTools(page);
  await page.getByRole('link', { name: new RegExp(name) }).click();
}

// ── Tools index ───────────────────────────────────────────────────────────────

test.describe('Tools index (/tools)', () => {
  test.beforeEach(async ({ page }) => {
    await navigateToTools(page);
  });

  test('renders page heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Tools', level: 1 })).toBeVisible();
  });

  test('lists the Branch Visualizer card', async ({ page }) => {
    await expect(page.getByRole('link', { name: /Branch Visualizer/ })).toBeVisible();
  });

  test('Branch Visualizer card navigates to the tool', async ({ page }) => {
    await page.getByRole('link', { name: /Branch Visualizer/ }).click();
    await expect(page.getByRole('heading', { name: 'Branch Visualizer', level: 1 })).toBeVisible();
  });
});

// ── Branch Visualizer ─────────────────────────────────────────────────────────

test.describe('Branch Visualizer (/tools/branch-visualizer)', () => {
  test.beforeEach(async ({ page }) => {
    await navigateToTool(page, 'Branch Visualizer');
  });

  test('renders the page title', async ({ page }) => {
    await expect(
      page.getByRole('heading', { name: 'Branch Visualizer', level: 1 }),
    ).toBeVisible();
  });

  test('renders an SVG with content', async ({ page }) => {
    // The static HTML already has small SVG icons (width ≤ 30px).
    // rough.js sets width on the canvas SVG to 400+ px; wait for that.
    await page.waitForFunction(
      () => Array.from(document.querySelectorAll('svg'))
              .some((s) => Number(s.getAttribute('width')) > 100),
      undefined,
      { timeout: 10_000 },
    );
  });

  test('editor panels for Branches and Commits are present', async ({ page }) => {
    await expect(page.getByText('Branches').first()).toBeVisible();
    await expect(page.getByText('Commits').first()).toBeVisible();
  });

  test('all export buttons are visible', async ({ page }) => {
    await expect(page.getByRole('button', { name: /Copy Link/i })).toBeVisible();
    await expect(page.getByRole('button', { name: 'SVG' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'PNG' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Download/i })).toBeVisible();
  });

  test('diagram state is encoded in the URL', async ({ page }) => {
    await page.waitForURL(/[?&]s=/, { timeout: 5_000 });
    expect(page.url()).toMatch(/[?&]s=/);
  });

  test('adding a branch increases the branch count', async ({ page }) => {
    const colorInputs = page.locator('input[type="color"]');
    // Wait for hydration — default state renders color pickers for each branch
    await colorInputs.first().waitFor();
    const before = await colorInputs.count();
    // First "+ Add" button belongs to the Branch editor
    await page.getByRole('button', { name: '+ Add' }).first().click();
    await expect(colorInputs).toHaveCount(before + 1);
  });

  test('adding a commit increases the commit count', async ({ page }) => {
    const idInputs = page.locator('input[placeholder="id"]');
    // Wait for hydration — default state renders id inputs for each commit
    await idInputs.first().waitFor();
    const before = await idInputs.count();
    // Second "+ Add" button belongs to the Commit editor
    await page.getByRole('button', { name: '+ Add' }).nth(1).click();
    await expect(idInputs).toHaveCount(before + 1);
  });

  test('shared URL encodes valid branch and commit data', async ({ page }) => {
    await page.waitForURL(/[?&]s=/, { timeout: 5_000 });
    const s = new URL(page.url()).searchParams.get('s');
    expect(s).toBeTruthy();
    // Decode and verify the payload contains branches and commits
    const state = JSON.parse(decodeURIComponent(atob(s!)));
    expect(Array.isArray(state.branches)).toBe(true);
    expect(state.branches.length).toBeGreaterThan(0);
    expect(Array.isArray(state.commits)).toBe(true);
    expect(state.commits.length).toBeGreaterThan(0);
  });
});

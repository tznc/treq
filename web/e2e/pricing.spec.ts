import { expect, test, type Page } from '@playwright/test';
import { PRICING_SCHEMA } from '../src/lib/pricing';

// The pricing page, reached from the home page through the navbar.
async function openPricing(page: Page) {
  await page.goto('/');
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Pricing' }).click();
  await expect(page.getByRole('heading', { name: 'Pricing', level: 1 })).toBeVisible();
  return page.getByRole('region', { name: 'Plans' });
}

test('lists Free, Pro and Team with their prices', async ({ page }) => {
  const plans = await openPricing(page);

  const free = plans.getByRole('article', { name: 'Free' });
  await expect(free).toContainText('$0');
  await expect(free).toContainText('gh');
  await expect(free).toContainText('personal API key');

  const pro = plans.getByRole('article', { name: 'Pro' });
  await expect(pro).toContainText('$15');
  await expect(pro).toContainText('/user/month');
  await expect(pro).toContainText('14-day free trial');
  await expect(pro).toContainText('Managed cloud workspace (private alpha)');
  await expect(pro).toContainText('Linear OAuth');
  await expect(pro).not.toContainText(/mobile/i);

  const team = plans.getByRole('article', { name: 'Team' });
  await expect(team).toContainText('$199');
  await expect(team).toContainText('5 members included, US$8 per extra');
  await expect(team).toContainText('Everything in Pro for every member');
  await expect(team).toContainText('Shared GitHub App installation');
});

test('points each plan at the right next step', async ({ page }) => {
  const plans = await openPricing(page);
  await expect(plans.getByRole('link', { name: 'Download Treq' })).toHaveAttribute(
    'href',
    'https://github.com/tznc/treq/releases/latest',
  );
  await expect(plans.getByRole('link', { name: 'Start 14-day free trial' })).toHaveAttribute(
    'href',
    '/dashboard?tab=subscription',
  );
  await expect(plans.getByRole('link', { name: 'Set up Team' })).toHaveAttribute(
    'href',
    '/dashboard?tab=team',
  );

  // Signed out, the Team tab asks you to sign in first.
  await plans.getByRole('link', { name: 'Set up Team' }).click();
  await expect(page.getByRole('heading', { name: 'Sign in to Treq' })).toBeVisible();
});

test('compares the three plans', async ({ page }) => {
  await openPricing(page);
  const table = page.getByRole('region', { name: 'Feature comparison' }).getByRole('table');
  await expect(table.getByRole('columnheader')).toHaveText(['Feature', 'Free', 'Pro', 'Team']);
  await expect(table.getByRole('row', { name: /Free trial/ })).toContainText('14 days');
});

test('answers what is paid, how the trial works and how Team seats count', async ({ page }) => {
  await openPricing(page);
  const faq = page.getByRole('region', { name: 'Frequently asked questions' });

  await faq.getByText('What is free, and what do I pay for?').click();
  await expect(faq).toContainText('servers');

  await faq.getByText('How does the Pro trial work?').click();
  await expect(faq).toContainText('card');
  await expect(faq).toContainText('Team has no trial');

  await faq.getByText('How do Team seats count?').click();
  await expect(faq).toContainText('Each member beyond 5 adds US$8 per month');
});

test('describes the three plans in the Product JSON-LD', () => {
  expect(PRICING_SCHEMA['@type']).toBe('Product');
  expect(
    PRICING_SCHEMA.offers.map((offer) => [offer.name, offer.price, offer.priceCurrency]),
  ).toEqual([
    ['Free', '0', 'USD'],
    ['Pro', '15', 'USD'],
    ['Team', '199', 'USD'],
  ]);
  expect(JSON.stringify(PRICING_SCHEMA)).not.toMatch(/mobile/i);
});

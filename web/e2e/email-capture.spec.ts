import { test, expect } from '@playwright/test';

const ARTICLE = '/learn/concepts/git/git-worktrees';

test.describe('email capture on Learn pages', () => {
  test('subscribes with the page path and confirms', async ({ page }) => {
    let body: Record<string, string> | null = null;
    await page.route(/\/rest\/v1\/rpc\/subscribe_newsletter/, async (route) => {
      body = route.request().postDataJSON();
      await route.fulfill({ status: 204, body: '' });
    });
    await page.goto(ARTICLE);

    const form = page.getByRole('form', { name: 'Get new guides by email' });
    await form.getByLabel('Email address').fill('reader@example.com');
    await form.getByRole('button', { name: 'Subscribe' }).click();

    await expect(form.getByText('Thanks. New guides will reach your inbox.')).toBeVisible();
    expect(body).toEqual({ p_email: 'reader@example.com', p_source_path: ARTICLE });
  });

  test('shows an error when the sign-up fails', async ({ page }) => {
    await page.route(/\/rest\/v1\/rpc\/subscribe_newsletter/, (route) =>
      route.fulfill({ status: 400, contentType: 'application/json', body: '{"message":"invalid email"}' }),
    );
    await page.goto(ARTICLE);

    const form = page.getByRole('form', { name: 'Get new guides by email' });
    await form.getByLabel('Email address').fill('reader@example.com');
    await form.getByRole('button', { name: 'Subscribe' }).click();

    await expect(form.getByRole('alert')).toHaveText('That did not work. Check the address and try again.');
  });
});

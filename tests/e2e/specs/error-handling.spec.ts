import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Error handling', () => {
  test('404 page renders for unknown routes', async ({ page }) => {
    const resp = await page.goto('/this-page-does-not-exist');
    expect(resp?.status()).toBe(404);
    await expect(page.locator('text=404')).toBeVisible();
    await expect(page.locator('text=Back to memories')).toBeVisible();
  });

  test('invalid memory ID shows error', async ({ page }) => {
    const resp = await page.goto('/memories/00000000-0000-0000-0000-000000000000');
    // Should return an error (404 or 500 depending on storage backend)
    expect(resp?.status()).toBeGreaterThanOrEqual(400);
  });
});

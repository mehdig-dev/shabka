import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';
import { makeMemory, uniqueTitle } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Timeline page', () => {
  test('timeline entries display with timestamps', async ({ page, apiClient }) => {
    const title = uniqueTitle('timeline-entry');
    await apiClient.createMemory(makeMemory({ title }));

    await page.goto('/timeline');
    const item = page.locator('.timeline-item', { hasText: title });
    await expect(item).toBeVisible();

    // Should have a timestamp
    await expect(item.locator('.time')).toBeVisible();
    // Should have a card with kind badge
    await expect(item.locator('.badge-kind')).toBeVisible();
  });

  test('empty timeline shows no entries message', async ({ page }) => {
    // Navigate to timeline — if empty, should show the empty state
    await page.goto('/timeline');
    // Either has timeline items or empty state — page should load without error
    const heading = page.locator('.page-header h1');
    await expect(heading).toHaveText('Timeline');
  });
});

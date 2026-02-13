import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';
import { makeMemory, uniqueTitle } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Search', () => {
  test('search results render with cards', async ({ page, apiClient }) => {
    const title = uniqueTitle('searchresult');
    await apiClient.createMemory(makeMemory({ title }));

    await page.goto(`/search?q=${encodeURIComponent(title)}`);
    await expect(page.locator('.card', { hasText: title })).toBeVisible();
    // Results count text
    await expect(page.locator('text=/\\d+ results? for/')).toBeVisible();
  });

  test('search highlighting wraps matches in <mark>', async ({ page, apiClient }) => {
    const keyword = uniqueTitle('highlight');
    await apiClient.createMemory(makeMemory({ title: keyword }));

    await page.goto(`/search?q=${encodeURIComponent(keyword)}`);
    // The highlighting script uses mark elements
    const marks = page.locator('mark');
    await expect(marks.first()).toBeVisible();
  });

  test('search page shows results count text', async ({ page }) => {
    const gibberish = 'zzz-nonexistent-query-' + Date.now();
    await page.goto(`/search?q=${encodeURIComponent(gibberish)}`);
    // With hash embeddings, vector search may still return results.
    // Verify the results count text renders (e.g. "N results for ...")
    await expect(page.locator('text=/\\d+ results? for/')).toBeVisible();
  });

  test('nav search form submits to search page', async ({ page }) => {
    const query = 'test-nav-search';
    await page.goto('/');
    await page.fill('#nav-search', query);
    await page.locator('nav .search-form button[type="submit"]').click();
    await page.waitForURL(/\/search\?q=/);
    expect(page.url()).toContain(`q=${query}`);
  });
});

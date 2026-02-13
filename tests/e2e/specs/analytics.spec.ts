import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';
import { makeMemory } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Analytics page', () => {
  test('stats cards are visible', async ({ page, apiClient }) => {
    // Ensure at least one memory exists
    await apiClient.createMemory(makeMemory());

    await page.goto('/analytics');

    // Check for stat cards — use exact text matching to avoid ambiguity
    await expect(page.getByText('Total Memories', { exact: true })).toBeVisible();
    await expect(page.getByText('Active', { exact: true })).toBeVisible();
    await expect(page.getByText('Archived', { exact: true })).toBeVisible();
    await expect(page.getByText('Relations', { exact: true })).toBeVisible();
  });

  test('Chart.js canvases are rendered', async ({ page, apiClient }) => {
    await apiClient.createMemory(makeMemory());

    await page.goto('/analytics');

    // Both chart canvases should exist
    await expect(page.locator('#kindChart')).toBeVisible();
    await expect(page.locator('#trendChart')).toBeVisible();
  });

  test('embedding info is displayed', async ({ page }) => {
    await page.goto('/analytics');

    // Embedding provider card
    await expect(page.locator('text=Embedding Provider')).toBeVisible();
  });
});

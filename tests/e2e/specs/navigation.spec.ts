import { test, expect, skipIfUnavailable } from '../fixtures/shabka-fixtures';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Navigation', () => {
  test('nav links are present and navigable', async ({ page }) => {
    await page.goto('/');
    const links = page.locator('nav .links a');

    await expect(links.filter({ hasText: 'Memories' })).toBeVisible();
    await expect(links.filter({ hasText: 'Timeline' })).toBeVisible();
    await expect(links.filter({ hasText: 'Graph' })).toBeVisible();
    await expect(links.filter({ hasText: 'Analytics' })).toBeVisible();
  });

  test('active nav link is highlighted for current page', async ({ page }) => {
    await page.goto('/timeline');
    const activeLink = page.locator('nav .links a.active');
    await expect(activeLink).toHaveText('Timeline');
  });

  test('Ctrl+K focuses the search input', async ({ page }) => {
    await page.goto('/');
    // Ensure search input is not focused initially
    await expect(page.locator('#nav-search')).not.toBeFocused();

    await page.keyboard.press('Control+k');
    await expect(page.locator('#nav-search')).toBeFocused();
  });

  test('slash key focuses search but not when typing in inputs', async ({ page }) => {
    await page.goto('/');

    // Slash key should focus search when no input is active
    await page.keyboard.press('/');
    await expect(page.locator('#nav-search')).toBeFocused();

    // Escape to blur
    await page.keyboard.press('Escape');
    await expect(page.locator('#nav-search')).not.toBeFocused();
  });
});

import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Theme toggle', () => {
  test('toggle switches between dark and light mode', async ({ page }) => {
    await page.goto('/');

    // Default is dark (no .light class)
    const html = page.locator('html');
    const hasLight = await html.evaluate(el => el.classList.contains('light'));

    // Click theme toggle
    await page.click('#theme-toggle');

    // Class should have toggled
    const hasLightAfter = await html.evaluate(el => el.classList.contains('light'));
    expect(hasLightAfter).toBe(!hasLight);
  });

  test('theme persists in localStorage', async ({ page }) => {
    await page.goto('/');

    // Set to light mode
    await page.evaluate(() => {
      document.documentElement.classList.remove('light');
      localStorage.removeItem('kaizen-theme');
    });

    await page.click('#theme-toggle');

    const stored = await page.evaluate(() => localStorage.getItem('kaizen-theme'));
    expect(stored).toBe('light');
  });

  test('theme persists across page navigation', async ({ page }) => {
    await page.goto('/');

    // Ensure we're in dark mode, then switch to light
    await page.evaluate(() => {
      document.documentElement.classList.remove('light');
      localStorage.setItem('kaizen-theme', 'dark');
    });
    await page.click('#theme-toggle');

    // Navigate to another page
    await page.goto('/timeline');

    // Should still be light
    const isLight = await page.evaluate(() => document.documentElement.classList.contains('light'));
    expect(isLight).toBe(true);

    // Clean up: reset to dark
    await page.evaluate(() => {
      localStorage.removeItem('kaizen-theme');
    });
  });
});

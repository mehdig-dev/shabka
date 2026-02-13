import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Memory form', () => {
  test('importance slider updates #imp-val display', async ({ page }) => {
    await page.goto('/memories/new');

    // Default should show 50
    await expect(page.locator('#imp-val')).toHaveText('50');

    // Move slider to 80%
    await page.locator('#importance').fill('0.8');
    await page.locator('#importance').dispatchEvent('input');
    await expect(page.locator('#imp-val')).toHaveText('80');
  });

  test('character counter updates on typing', async ({ page }) => {
    await page.goto('/memories/new');
    const counter = page.locator('#char-count');

    // Initially empty (no text or "0 chars" depending on implementation)
    await page.fill('#content', '');
    const initial = await counter.textContent();
    expect(initial).toBe('');

    // Type something
    await page.fill('#content', 'Hello world');
    await expect(counter).toHaveText('11 chars');
  });

  test('tab key inserts spaces in textarea', async ({ page }) => {
    await page.goto('/memories/new');
    await page.click('#content');
    await page.keyboard.type('line1');
    await page.keyboard.press('Tab');
    await page.keyboard.type('indented');

    const value = await page.locator('#content').inputValue();
    expect(value).toContain('line1  indented');
  });

  test('required fields prevent empty submission', async ({ page }) => {
    await page.goto('/memories/new');
    // Don't fill anything, just submit via the form button
    await page.click('.memory-form button[type="submit"]');

    // Browser validation should prevent navigation — still on /memories/new
    expect(page.url()).toContain('/memories/new');
  });
});

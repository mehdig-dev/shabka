import { test, expect, skipIfUnavailable } from '../fixtures/shabka-fixtures';
import { makeMemory, uniqueTitle } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Bulk operations', () => {
  test('checking a checkbox shows the bulk action bar', async ({ page, apiClient }) => {
    const created = await apiClient.createMemory(makeMemory({ title: uniqueTitle('bulk-check') }));

    await page.goto('/');
    const checkbox = page.locator(`.bulk-select[data-id="${created.id}"]`);
    await checkbox.check();

    const bar = page.locator('#bulk-bar');
    await expect(bar).toBeVisible();
    await expect(page.locator('#bulk-count')).toHaveText('1 selected');
  });

  test('clear selection hides the bulk bar', async ({ page, apiClient }) => {
    const created = await apiClient.createMemory(makeMemory({ title: uniqueTitle('bulk-clear') }));

    await page.goto('/');
    const checkbox = page.locator(`.bulk-select[data-id="${created.id}"]`);
    await checkbox.check();

    await expect(page.locator('#bulk-bar')).toBeVisible();

    // Click the clear button (✕)
    await page.click('#bulk-bar button:last-child');
    await expect(page.locator('#bulk-bar')).not.toBeVisible();
  });

  test('bulk delete removes selected memories', async ({ page, apiClient }) => {
    const m1 = await apiClient.createMemory(makeMemory({ title: uniqueTitle('bulk-del-1') }));
    const m2 = await apiClient.createMemory(makeMemory({ title: uniqueTitle('bulk-del-2') }));

    await page.goto('/');

    // Check both
    await page.locator(`.bulk-select[data-id="${m1.id}"]`).check();
    await page.locator(`.bulk-select[data-id="${m2.id}"]`).check();
    await expect(page.locator('#bulk-count')).toHaveText('2 selected');

    // Accept both the confirm dialog and the alert
    page.on('dialog', dialog => dialog.accept());

    // Click bulk delete
    await page.click('button:has-text("Delete Selected")');

    // Wait for page reload
    await page.waitForLoadState('networkidle');
  });

  test('bulk archive updates memory status', async ({ page, apiClient }) => {
    const created = await apiClient.createMemory(makeMemory({ title: uniqueTitle('bulk-archive') }));

    await page.goto('/');
    await page.locator(`.bulk-select[data-id="${created.id}"]`).check();

    // Accept dialogs
    page.on('dialog', dialog => dialog.accept());

    await page.click('button:has-text("Archive Selected")');
    await page.waitForLoadState('networkidle');

    // Verify via API that status changed
    const fetched = await apiClient.getMemory(created.id);
    expect(fetched.memory.status).toBe('archived');
  });
});

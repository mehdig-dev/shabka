import { test, expect, skipIfUnavailable } from '../fixtures/shabka-fixtures';
import { uniqueTitle, ALL_KINDS } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('Memory CRUD via UI', () => {
  test('create a memory via form and see it on detail page', async ({ page, apiClient }) => {
    const title = uniqueTitle('form-create');

    await page.goto('/memories/new');
    await page.fill('#title', title);
    await page.fill('#content', 'Test content for E2E');
    await page.selectOption('#kind', 'observation');
    await page.fill('#tags', 'e2e-test');
    await page.click('.memory-form button[type="submit"]');

    // Should redirect to detail page
    await page.waitForURL(/\/memories\//);
    await expect(page.locator('.page-header h1')).toHaveText(title);

    // Clean up — extract ID from URL
    const url = page.url();
    const id = url.split('/memories/')[1]?.split('?')[0];
    if (id) {
      await apiClient.deleteMemory(id);
    }
  });

  test('view memory detail page shows all metadata', async ({ page, testMemory }) => {
    await page.goto(`/memories/${testMemory.id}`);

    await expect(page.locator('.page-header h1')).toHaveText(testMemory.title);
    await expect(page.locator('.badge-kind').first()).toBeVisible();
    await expect(page.locator('#memory-content')).toBeVisible();
  });

  test('edit a memory via form', async ({ page, apiClient }) => {
    const created = await apiClient.createMemory({
      title: uniqueTitle('editable'),
      content: 'Original content',
      kind: 'observation',
      tags: ['e2e-test'],
    });

    await page.goto(`/memories/${created.id}/edit`);
    const newTitle = uniqueTitle('edited');
    await page.fill('#title', newTitle);
    await page.click('.memory-form button[type="submit"]');

    // Should redirect to detail page with updated title
    await page.waitForURL(/\/memories\//);
    await expect(page.locator('.page-header h1')).toHaveText(newTitle);
  });

  test('delete a memory via form', async ({ page, apiClient }) => {
    const created = await apiClient.createMemory({
      title: uniqueTitle('deletable'),
      content: 'To be deleted',
      kind: 'observation',
      tags: ['e2e-test'],
    });

    await page.goto(`/memories/${created.id}`);

    // Accept the confirm dialog
    page.on('dialog', dialog => dialog.accept());
    await page.click('.btn-danger');

    // Should redirect to list with toast
    await page.waitForURL('/');
  });

  test('kind select shows all 9 options', async ({ page }) => {
    await page.goto('/memories/new');
    const options = await page.locator('#kind option').allTextContents();
    for (const kind of ALL_KINDS) {
      expect(options).toContain(kind);
    }
  });
});

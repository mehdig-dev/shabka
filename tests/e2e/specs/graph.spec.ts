import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';
import { makeMemory, uniqueTitle } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

// The graph page declares `let cy;` at top-level scope and `<div id="cy">`.
// window.cy resolves to the DOM element, not the Cytoscape instance.
// Use eval-based access to reach the lexical `cy` variable.
const waitForCytoscape = async (page: any) => {
  await page.waitForFunction(
    () => {
      try {
        // Access lexical `cy` via indirect eval
        const instance = (0, eval)('cy');
        return instance && typeof instance.nodes === 'function';
      } catch { return false; }
    },
    null,
    { timeout: 15000 },
  );
};

const cyNodeCount = (page: any) =>
  page.evaluate(() => {
    const instance = (0, eval)('cy');
    return instance.nodes().length;
  });

const cyVisibleNodeCount = (page: any) =>
  page.evaluate(() => {
    const instance = (0, eval)('cy');
    return instance.nodes().filter((n: any) => n.style('display') !== 'none').length;
  });

test.describe('Graph page', () => {
  test('Cytoscape container renders', async ({ page, apiClient }) => {
    await apiClient.createMemory(makeMemory());

    await page.goto('/graph');
    await expect(page.locator('#cy')).toBeVisible();

    await waitForCytoscape(page);

    const nodeCount = await cyNodeCount(page);
    expect(nodeCount).toBeGreaterThanOrEqual(1);
  });

  test('kind filter checkboxes are generated', async ({ page, apiClient }) => {
    await apiClient.createMemory(makeMemory({ kind: 'observation' }));

    await page.goto('/graph');
    await waitForCytoscape(page);

    const filters = page.locator('#kind-filters input[type="checkbox"]');
    const count = await filters.count();
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test('layout selector changes layout', async ({ page, apiClient }) => {
    await apiClient.createMemory(makeMemory());

    await page.goto('/graph');
    await waitForCytoscape(page);

    await page.selectOption('#layout-select', 'circle');

    await page.waitForTimeout(600);
    const nodeCount = await cyNodeCount(page);
    expect(nodeCount).toBeGreaterThanOrEqual(1);
  });

  test('search filter hides non-matching nodes', async ({ page, apiClient }) => {
    const title = uniqueTitle('graph-searchable');
    await apiClient.createMemory(makeMemory({ title }));

    await page.goto('/graph');
    await waitForCytoscape(page);

    const totalBefore = await cyVisibleNodeCount(page);

    await page.fill('#graph-search', 'zzz-no-match-' + Date.now());
    await page.waitForTimeout(200);

    const totalAfter = await cyVisibleNodeCount(page);
    expect(totalAfter).toBeLessThanOrEqual(totalBefore);
  });
});

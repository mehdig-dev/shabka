import { test, expect, skipIfUnavailable } from '../fixtures/kaizen-fixtures';
import { makeMemory, uniqueTitle } from '../helpers/test-data';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test.describe('REST API', () => {
  test('POST then GET a memory', async ({ apiClient }) => {
    const input = makeMemory({ title: uniqueTitle('api-create') });
    const created = await apiClient.createMemory(input);

    expect(created.action).toBe('added');
    expect(created.id).toBeTruthy();
    expect(created.title).toBe(input.title);

    const fetched = await apiClient.getMemory(created.id);
    expect(fetched.memory.title).toBe(input.title);
    expect(fetched.memory.content).toBe(input.content);
    expect(fetched.memory.kind).toBe(input.kind);
    expect(fetched.memory.tags).toContain('e2e-test');
  });

  test('PUT updates a memory', async ({ apiClient }) => {
    const created = await apiClient.createMemory(makeMemory());
    const newTitle = uniqueTitle('updated');

    const updated = await apiClient.updateMemory(created.id, { title: newTitle });
    expect(updated.title).toBe(newTitle);

    const fetched = await apiClient.getMemory(created.id);
    expect(fetched.memory.title).toBe(newTitle);
  });

  test('DELETE removes a memory', async ({ apiClient }) => {
    const created = await apiClient.createMemory(makeMemory());
    await apiClient.deleteMemory(created.id);

    await expect(async () => {
      await apiClient.getMemory(created.id);
    }).rejects.toThrow(/404/);
  });

  test('GET /api/v1/search returns results', async ({ apiClient }) => {
    const title = uniqueTitle('searchable');
    await apiClient.createMemory(makeMemory({ title }));

    const results = await apiClient.search(title);
    expect(results.length).toBeGreaterThanOrEqual(1);
    expect(results.some(r => r.title === title)).toBe(true);
  });

  test('GET /api/v1/stats returns correct shape', async ({ apiClient }) => {
    // Ensure at least one memory exists
    await apiClient.createMemory(makeMemory());

    const stats = await apiClient.stats();
    expect(stats.total_memories).toBeGreaterThanOrEqual(1);
    expect(stats.by_kind).toBeInstanceOf(Array);
    expect(stats.by_status).toHaveProperty('active');
    expect(stats.by_status).toHaveProperty('archived');
    expect(stats.by_status).toHaveProperty('superseded');
    expect(typeof stats.embedding_provider).toBe('string');
    expect(typeof stats.embedding_model).toBe('string');
    expect(typeof stats.embedding_dimensions).toBe('number');
  });

  test('POST /api/v1/memories/bulk/delete removes multiple', async ({ apiClient }) => {
    const m1 = await apiClient.createMemory(makeMemory());
    const m2 = await apiClient.createMemory(makeMemory());

    const result = await apiClient.bulkDelete([m1.id, m2.id]);
    expect(result.processed).toBe(2);
    expect(result.errors).toBe(0);
  });
});

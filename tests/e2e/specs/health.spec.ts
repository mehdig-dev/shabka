import { test, expect, skipIfUnavailable } from '../fixtures/shabka-fixtures';

test.beforeEach(async () => {
  await skipIfUnavailable();
});

test('GET /health returns JSON with status, helix_db, and embedding_provider', async ({ request }) => {
  const resp = await request.get('/health');
  expect(resp.ok()).toBe(true);

  const body = await resp.json();
  expect(body).toHaveProperty('status', 'ok');
  expect(body).toHaveProperty('helix_db', 'connected');
  expect(body).toHaveProperty('embedding_provider');
  expect(typeof body.embedding_provider).toBe('string');
});

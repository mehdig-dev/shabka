import { test } from '@playwright/test';

const BASE_URL = process.env.BASE_URL || 'http://localhost:37737';

/**
 * Check if the Shabka web dashboard is reachable.
 * Returns null if healthy, or a skip reason string.
 */
export async function checkServices(): Promise<string | null> {
  try {
    const resp = await fetch(`${BASE_URL}/health`, { signal: AbortSignal.timeout(3000) });
    if (!resp.ok) {
      const body = await resp.json().catch(() => ({}));
      if (body.helix_db === 'unavailable') {
        return 'HelixDB unavailable — start with: just db';
      }
      return `Health check returned ${resp.status}`;
    }
    return null;
  } catch (e) {
    return `Web dashboard unreachable at ${BASE_URL} — start with: just db && just web`;
  }
}

/**
 * Use this in test.beforeAll() to skip the entire suite
 * if the required services are not running.
 */
export async function skipIfUnavailable(): Promise<void> {
  const reason = await checkServices();
  if (reason) {
    test.skip(true, reason);
  }
}

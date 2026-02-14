import { test as base } from '@playwright/test';
import { ApiClient, type CreateResponse } from '../helpers/api-client';
import { makeMemory, type CreateMemoryInput } from '../helpers/test-data';
import { skipIfUnavailable } from '../helpers/service-guard';

type ShabkaFixtures = {
  /** Pre-configured API client with auto-cleanup */
  apiClient: ApiClient;
  /** Create a test memory that is automatically deleted after the test */
  testMemory: CreateResponse;
};

export const test = base.extend<ShabkaFixtures>({
  apiClient: async ({}, use) => {
    const client = new ApiClient();
    await use(client);
    await client.cleanup();
  },

  testMemory: async ({ apiClient }, use) => {
    const result = await apiClient.createMemory(makeMemory());
    await use(result);
    // cleanup handled by apiClient
  },
});

export { expect } from '@playwright/test';
export { skipIfUnavailable };

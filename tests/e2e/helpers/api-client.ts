const BASE_URL = process.env.BASE_URL || 'http://localhost:37737';

export interface CreateMemoryInput {
  title: string;
  content: string;
  kind: string;
  tags?: string[];
  importance?: number;
}

export interface MemoryResponse {
  memory: Memory;
  relations: MemoryRelation[];
}

export interface Memory {
  id: string;
  title: string;
  content: string;
  kind: string;
  tags: string[];
  importance: number;
  status: string;
  source: string;
  scope: string;
  created_by: string;
  created_at: string;
  updated_at: string;
  accessed_at: string;
  access_count: number;
  privacy: string;
}

export interface MemoryRelation {
  source_id: string;
  target_id: string;
  relation_type: string;
  strength: number;
}

export interface CreateResponse {
  action: string;
  id: string;
  title: string;
  superseded_id?: string;
  similarity?: number;
}

export interface StatsResponse {
  total_memories: number;
  by_kind: { kind: string; count: number }[];
  by_status: { active: number; archived: number; superseded: number };
  total_relations: number;
  embedding_provider: string;
  embedding_model: string;
  embedding_dimensions: number;
}

export interface BulkResult {
  processed: number;
  errors: number;
}

export interface TimelineEntry {
  id: string;
  title: string;
  kind: string;
  importance: number;
  created_at: string;
  created_by: string;
  related_count: number;
  privacy: string;
}

export interface SearchResult {
  id: string;
  title: string;
  kind: string;
  score: number;
  tags: string[];
  created_at: string;
}

export class ApiClient {
  private baseUrl: string;
  /** Track created memory IDs for cleanup */
  private createdIds: string[] = [];

  constructor(baseUrl?: string) {
    this.baseUrl = baseUrl || BASE_URL;
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const opts: RequestInit = {
      method,
      headers: { 'Content-Type': 'application/json' },
    };
    if (body !== undefined) {
      opts.body = JSON.stringify(body);
    }
    const resp = await fetch(`${this.baseUrl}${path}`, opts);
    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(`API ${method} ${path} failed (${resp.status}): ${text}`);
    }
    return resp.json() as Promise<T>;
  }

  async createMemory(input: CreateMemoryInput): Promise<CreateResponse> {
    const result = await this.request<CreateResponse>('POST', '/api/v1/memories', input);
    if (result.action !== 'skipped') {
      this.createdIds.push(result.id);
    }
    return result;
  }

  async getMemory(id: string): Promise<MemoryResponse> {
    return this.request<MemoryResponse>('GET', `/api/v1/memories/${id}`);
  }

  async updateMemory(id: string, update: Partial<Pick<Memory, 'title' | 'content' | 'tags' | 'importance' | 'status'>>): Promise<Memory> {
    return this.request<Memory>('PUT', `/api/v1/memories/${id}`, update);
  }

  async deleteMemory(id: string): Promise<void> {
    await this.request<unknown>('DELETE', `/api/v1/memories/${id}`);
    this.createdIds = this.createdIds.filter(cid => cid !== id);
  }

  async listMemories(params?: { kind?: string; limit?: number }): Promise<TimelineEntry[]> {
    const qs = new URLSearchParams();
    if (params?.kind) qs.set('kind', params.kind);
    if (params?.limit) qs.set('limit', String(params.limit));
    const query = qs.toString() ? `?${qs}` : '';
    return this.request<TimelineEntry[]>('GET', `/api/v1/memories${query}`);
  }

  async search(q: string, opts?: { kind?: string; limit?: number; tag?: string }): Promise<SearchResult[]> {
    const qs = new URLSearchParams({ q });
    if (opts?.kind) qs.set('kind', opts.kind);
    if (opts?.limit) qs.set('limit', String(opts.limit));
    if (opts?.tag) qs.set('tag', opts.tag);
    return this.request<SearchResult[]>('GET', `/api/v1/search?${qs}`);
  }

  async stats(): Promise<StatsResponse> {
    return this.request<StatsResponse>('GET', '/api/v1/stats');
  }

  async timeline(limit?: number): Promise<TimelineEntry[]> {
    const qs = limit ? `?limit=${limit}` : '';
    return this.request<TimelineEntry[]>('GET', `/api/v1/timeline${qs}`);
  }

  async addRelation(sourceId: string, targetId: string, relationType: string, strength = 0.5): Promise<unknown> {
    return this.request<unknown>('POST', `/api/v1/memories/${sourceId}/relate`, {
      target_id: targetId,
      relation_type: relationType,
      strength,
    });
  }

  async getRelations(id: string): Promise<MemoryRelation[]> {
    return this.request<MemoryRelation[]>('GET', `/api/v1/memories/${id}/relations`);
  }

  async getHistory(id: string): Promise<unknown[]> {
    return this.request<unknown[]>('GET', `/api/v1/memories/${id}/history`);
  }

  async bulkArchive(ids: string[]): Promise<BulkResult> {
    return this.request<BulkResult>('POST', '/api/v1/memories/bulk/archive', { ids });
  }

  async bulkDelete(ids: string[]): Promise<BulkResult> {
    return this.request<BulkResult>('POST', '/api/v1/memories/bulk/delete', { ids });
  }

  /** Delete all memories created during this session */
  async cleanup(): Promise<void> {
    for (const id of [...this.createdIds]) {
      try {
        await this.deleteMemory(id);
      } catch {
        // Already deleted or doesn't exist
      }
    }
    this.createdIds = [];
  }
}

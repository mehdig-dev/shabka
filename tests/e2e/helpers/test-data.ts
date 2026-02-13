import { randomUUID } from 'crypto';
import type { CreateMemoryInput } from './api-client';

/** Generate a unique title prefixed with E2E- for easy identification */
export function uniqueTitle(base = 'Test'): string {
  const short = randomUUID().slice(0, 8);
  return `E2E-${short} ${base}`;
}

/** All valid memory kinds */
export const ALL_KINDS = [
  'observation', 'decision', 'pattern', 'error',
  'fix', 'preference', 'fact', 'lesson', 'todo',
] as const;

/** Create a memory input with sensible defaults and a unique title */
export function makeMemory(overrides?: Partial<CreateMemoryInput>): CreateMemoryInput {
  return {
    title: uniqueTitle(overrides?.kind || 'observation'),
    content: 'E2E test content — this memory was created by an automated test.',
    kind: 'observation',
    tags: ['e2e-test'],
    importance: 0.5,
    ...overrides,
  };
}

/** Create a memory with markdown content for rendering tests */
export function makeMarkdownMemory(): CreateMemoryInput {
  return makeMemory({
    title: uniqueTitle('markdown'),
    content: [
      '# Heading One',
      '',
      'A paragraph with **bold** and *italic* text.',
      '',
      '- Item one',
      '- Item two',
      '- Item three',
      '',
      '```rust',
      'fn main() {',
      '    println!("hello");',
      '}',
      '```',
    ].join('\n'),
  });
}

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseIterationDetails } from '../../src/parsers/iterationDetails';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('parseIterationDetails', () => {
  const fixture = loadFixture('iteration-details-sample.md');
  const entries = parseIterationDetails(fixture);

  it('parses correct number of iteration entries', () => {
    // Sample has iterations: 2,3,4,5,6,7,8,9,10,11,12,14,16,20,21,23,24,25
    expect(entries.length).toBe(18);
  });

  it('extracts iteration number and timestamp', () => {
    expect(entries[0]!.iteration).toBe(2);
    expect(entries[0]!.timestamp).toBe('19:51:13');
    expect(entries[entries.length - 1]!.iteration).toBe(25);
    expect(entries[entries.length - 1]!.timestamp).toBe('20:38:24');
  });

  it('extracts silence duration from trigger source', () => {
    // Iteration 2: "Slow, silence=10002ms [BLOCK STILL EXECUTING]"
    expect(entries[0]!.silenceDurationMs).toBe(10002);
  });

  it('detects BLOCK STILL EXECUTING flag', () => {
    // Iteration 2 has [BLOCK STILL EXECUTING]
    expect(entries[0]!.blockExecuting).toBe(true);
    // Iteration 3 has empty trigger
    expect(entries[1]!.blockExecuting).toBe(false);
  });

  it('handles entries with empty trigger field', () => {
    // Iteration 3 has **Trigger:** with empty value
    expect(entries[1]!.triggerSource).toBeNull();
    expect(entries[1]!.silenceDurationMs).toBeNull();
    expect(entries[1]!.blockExecuting).toBe(false);
  });

  it('handles entries with no trigger field at all', () => {
    // Iteration 10 (index 8) has no Trigger field — just Action and Note
    const iter10 = entries.find((e) => e.iteration === 10);
    expect(iter10).toBeDefined();
    expect(iter10!.triggerSource).toBeNull();
    expect(iter10!.action).toBe('checkpoint');
  });

  it('extracts action field', () => {
    expect(entries[0]!.action).toBe('instruction');
    const iter10 = entries.find((e) => e.iteration === 10);
    expect(iter10!.action).toBe('checkpoint');
    const iter25 = entries.find((e) => e.iteration === 25);
    expect(iter25!.action).toBe('done');
  });

  it('extracts instruction text', () => {
    expect(entries[0]!.instruction).toContain('Now build the core wizard');
  });

  it('extracts notes', () => {
    const iter25 = entries.find((e) => e.iteration === 25);
    expect(iter25!.note).toContain('Built Clarify career planning tool');
  });

  it('handles run separator lines', () => {
    const withSeparator = `---

# Run: PRD.md (2026-03-28 11:07:45)

## Iteration 1 [11:09:52]

**Action:** wait
**Note:** Agent responded GLASS_WAIT

## Iteration 2 [11:09:57]

**Trigger:** Slow, silence=60006ms [BLOCK STILL EXECUTING]
**Action:** instruction
**Agent instruction:** Do something
`;
    const result = parseIterationDetails(withSeparator);
    expect(result).toHaveLength(2);
    expect(result[0]!.iteration).toBe(1);
    expect(result[0]!.action).toBe('wait');
    expect(result[1]!.iteration).toBe(2);
    expect(result[1]!.silenceDurationMs).toBe(60006);
    expect(result[1]!.blockExecuting).toBe(true);
  });

  it('handles entries with files changed', () => {
    const withFiles = `## Iteration 5 [12:00:00]

**Action:** instruction
**Agent instruction:** Fix the bug
**Files changed:** src/main.ts, src/utils.ts, tests/main.test.ts
`;
    const result = parseIterationDetails(withFiles);
    expect(result).toHaveLength(1);
    expect(result[0]!.filesChanged).toEqual([
      'src/main.ts',
      'src/utils.ts',
      'tests/main.test.ts',
    ]);
  });

  it('defaults filesChanged to empty array when missing', () => {
    expect(entries[0]!.filesChanged).toEqual([]);
  });
});

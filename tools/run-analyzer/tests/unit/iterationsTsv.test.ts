import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseIterationsTsv } from '../../src/parsers/iterationsTsv';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('parseIterationsTsv', () => {
  const fixture = loadFixture('iterations-sample.tsv');
  const entries = parseIterationsTsv(fixture);

  it('parses all data rows, skipping header', () => {
    expect(entries).toHaveLength(10);
  });

  it('parses iteration numbers correctly', () => {
    expect(entries[0]!.iteration).toBe(1);
    expect(entries[entries.length - 1]!.iteration).toBe(25);
  });

  it('handles all status types', () => {
    const statuses = entries.map((e) => e.status);
    expect(statuses).toContain('baseline');
    expect(statuses).toContain('instruction');
    expect(statuses).toContain('keep');
    expect(statuses).toContain('stuck');
    expect(statuses).toContain('checkpoint');
    expect(statuses).toContain('revert');
    expect(statuses).toContain('wait');
    expect(statuses).toContain('complete');
  });

  it('extracts commit hashes', () => {
    const withCommit = entries.find((e) => e.iteration === 5);
    expect(withCommit!.commit).toBe('a1b2c3d');
    const noCommit = entries.find((e) => e.iteration === 3);
    expect(noCommit!.commit).toBe('');
  });

  it('extracts feature descriptions', () => {
    const entry = entries.find((e) => e.iteration === 3);
    expect(entry!.feature).toBe('Core wizard infrastructure');
  });

  it('extracts description text', () => {
    const entry = entries.find((e) => e.iteration === 7);
    expect(entry!.description).toBe('Agent retried 4 times without progress');
  });

  it('handles empty/missing columns', () => {
    // Iteration 1 has empty commit, feature, metric columns
    const entry = entries.find((e) => e.iteration === 1);
    expect(entry!.commit).toBe('');
    expect(entry!.feature).toBe('');
    expect(entry!.metric).toBe('');
  });

  it('skips run separator rows', () => {
    const withSeparator = `iteration\tcommit\tfeature\tmetric\tstatus\tdescription
--- Run: run-123 ---
1\t\t\t\tbaseline\tTest baseline
--- Run: run-456 ---
2\tabc123\tfeat\t\tinstruction\tDo something
`;
    const result = parseIterationsTsv(withSeparator);
    expect(result).toHaveLength(2);
    expect(result[0]!.iteration).toBe(1);
    expect(result[1]!.iteration).toBe(2);
  });

  it('handles TSV with only header row', () => {
    const headerOnly = 'iteration\tcommit\tfeature\tmetric\tstatus\tdescription\n';
    const result = parseIterationsTsv(headerOnly);
    expect(result).toHaveLength(0);
  });
});

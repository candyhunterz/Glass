import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRunMetrics } from '../../src/parsers/runMetrics';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('parseRunMetrics', () => {
  const fixture = loadFixture('run-metrics-sample.toml');
  const metrics = parseRunMetrics(fixture);

  it('parses multiple runs', () => {
    expect(metrics.runs).toHaveLength(2);
  });

  it('extracts run IDs', () => {
    expect(metrics.runs[0]!.run_id).toBe('run-1774683967');
    expect(metrics.runs[1]!.run_id).toBe('run-1774666152');
  });

  it('extracts numeric fields', () => {
    const run = metrics.runs[0]!;
    expect(run.iterations).toBe(21);
    expect(run.duration_secs).toBe(1983);
    expect(run.stuck_rate).toBeCloseTo(0.0476, 3);
    expect(run.checkpoint_rate).toBeCloseTo(0.0476, 3);
    expect(run.revert_rate).toBe(0);
    expect(run.waste_rate).toBe(0);
  });

  it('extracts completion string', () => {
    expect(metrics.runs[0]!.completion).toContain('Complete git-stats React dashboard');
  });

  it('extracts rule firings arrays', () => {
    const firings = metrics.runs[0]!.rule_firings;
    expect(firings).toHaveLength(3);
    expect(firings[0]!.rule_id).toBe('uncommitted-drift');
    expect(firings[0]!.action).toBe('force_commit');
    expect(firings[0]!.firing_count).toBe(250);
    expect(firings[1]!.rule_id).toBe('revert-rate');
    expect(firings[1]!.firing_count).toBe(260);
    expect(firings[2]!.firing_count).toBe(0);
  });

  it('handles runs with no rule firings', () => {
    const run2 = metrics.runs[1]!;
    expect(run2.rule_firings).toHaveLength(0);
    expect(run2.iterations).toBe(25);
    expect(run2.duration_secs).toBe(2951);
  });

  it('handles single-run TOML', () => {
    const single = `
[[runs]]
run_id = "run-single"
project_root = "/tmp"
iterations = 5
duration_secs = 300
revert_rate = 0.1
stuck_rate = 0.2
waste_rate = 0.05
checkpoint_rate = 0.0
completion = "done"
prd_items_completed = 3
prd_items_total = 5
kickoff_duration_secs = 10
rule_firings = []
`;
    const result = parseRunMetrics(single);
    expect(result.runs).toHaveLength(1);
    expect(result.runs[0]!.run_id).toBe('run-single');
    expect(result.runs[0]!.revert_rate).toBeCloseTo(0.1);
    expect(result.runs[0]!.prd_items_completed).toBe(3);
  });
});

import { describe, it, expect, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { useDataStore } from '../../src/stores/dataStore';
import {
  efficiencyScore,
  stallRate,
  waitRatio,
  avgIterationDuration,
} from '../../src/utils/analytics';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('Cross-run comparison integration', () => {
  beforeEach(() => {
    useDataStore.getState().reset();
  });

  it('loads metrics with 2 runs and verifies both are accessible', () => {
    useDataStore.getState().loadFiles({
      'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    });
    const state = useDataStore.getState();

    expect(state.runMetrics).not.toBeNull();
    const runs = state.runMetrics!.runs;
    expect(runs).toHaveLength(2);

    expect(runs[0]!.run_id).toBe('run-1774683967');
    expect(runs[1]!.run_id).toBe('run-1774666152');
  });

  it('verifies rate values for trend comparison', () => {
    useDataStore.getState().loadFiles({
      'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    });
    const runs = useDataStore.getState().runMetrics!.runs;

    // Run 1 has non-zero stuck and checkpoint rates
    expect(runs[0]!.stuck_rate).toBeCloseTo(0.0476, 3);
    expect(runs[0]!.checkpoint_rate).toBeCloseTo(0.0476, 3);
    expect(runs[0]!.waste_rate).toBe(0);
    expect(runs[0]!.revert_rate).toBe(0);

    // Run 2 has all zero rates except checkpoint
    expect(runs[1]!.stuck_rate).toBe(0);
    expect(runs[1]!.revert_rate).toBe(0);
    expect(runs[1]!.waste_rate).toBe(0);
    expect(runs[1]!.checkpoint_rate).toBeCloseTo(0.04);
  });

  it('verifies duration trend across runs', () => {
    useDataStore.getState().loadFiles({
      'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    });
    const runs = useDataStore.getState().runMetrics!.runs;

    expect(runs[0]!.duration_secs).toBe(1983);
    expect(runs[1]!.duration_secs).toBe(2951);
    // Second run was longer
    expect(runs[1]!.duration_secs).toBeGreaterThan(runs[0]!.duration_secs);
  });

  it('verifies iteration counts for trend', () => {
    useDataStore.getState().loadFiles({
      'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    });
    const runs = useDataStore.getState().runMetrics!.runs;

    expect(runs[0]!.iterations).toBe(21);
    expect(runs[1]!.iterations).toBe(25);
  });

  it('verifies rule firings differ between runs', () => {
    useDataStore.getState().loadFiles({
      'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    });
    const runs = useDataStore.getState().runMetrics!.runs;

    expect(runs[0]!.rule_firings).toHaveLength(3);
    expect(runs[1]!.rule_firings).toHaveLength(0);
  });

  it('computes derived analytics from loaded data', () => {
    useDataStore.getState().loadFiles({
      'run-report-sample.md': loadFixture('run-report-sample.md'),
      'iteration-details.md': loadFixture('iteration-details-sample.md'),
      'iterations.tsv': loadFixture('iterations-sample.tsv'),
      'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    });
    const state = useDataStore.getState();
    const report = state.runReports['run-report-sample.md']!;

    // Efficiency: (3 * 10) / (25 + 0 + 0) = 1.2
    expect(efficiencyScore(report)).toBeCloseTo(1.2);

    // Stall rate from iteration details
    const stall = stallRate(state.iterationDetails);
    expect(stall).toBeGreaterThanOrEqual(0);
    expect(stall).toBeLessThanOrEqual(1);

    // Wait ratio from TSV
    const wr = waitRatio(state.tsvEntries);
    // Fixture has 1 wait out of 10 entries = 0.1
    expect(wr).toBeCloseTo(0.1);

    // Avg iteration duration from metrics
    const runs = state.runMetrics!.runs;
    const avgDur = avgIterationDuration(runs[0]!.duration_secs, runs[0]!.iterations);
    expect(avgDur).toBeCloseTo(1983 / 21, 1);
  });
});

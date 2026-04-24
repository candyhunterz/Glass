import { describe, it, expect, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { useDataStore } from '../../src/stores/dataStore';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

function loadAllFixtures(): Record<string, string> {
  return {
    'run-report-sample.md': loadFixture('run-report-sample.md'),
    'iteration-details.md': loadFixture('iteration-details-sample.md'),
    'iterations.tsv': loadFixture('iterations-sample.tsv'),
    'run-metrics.toml': loadFixture('run-metrics-sample.toml'),
    'rules.toml': loadFixture('rules-sample.toml'),
    'tuning-history.toml': loadFixture('tuning-history-sample.toml'),
  };
}

describe('Full data load integration', () => {
  beforeEach(() => {
    useDataStore.getState().reset();
  });

  it('populates all store fields from fixture files', () => {
    const files = loadAllFixtures();
    useDataStore.getState().loadFiles(files);
    const state = useDataStore.getState();

    expect(state.isLoaded).toBe(true);
    expect(state.isLoading).toBe(false);
    expect(state.error).toBeNull();

    // Run reports
    expect(Object.keys(state.runReports)).toHaveLength(1);
    expect(state.runReports['run-report-sample.md']).toBeDefined();
    expect(state.runReports['run-report-sample.md']!.iterations).toBe(25);

    // Iteration details
    expect(state.iterationDetails.length).toBeGreaterThan(0);

    // TSV entries
    expect(state.tsvEntries.length).toBe(10);

    // Run metrics
    expect(state.runMetrics).not.toBeNull();
    expect(state.runMetrics!.runs).toHaveLength(2);

    // Rules
    expect(state.rules).toHaveLength(2);

    // Tuning history
    expect(state.tuningHistory).not.toBeNull();
    expect(state.tuningHistory!.snapshots).toHaveLength(1);
    expect(state.tuningHistory!.pending).not.toBeNull();

    // Raw files preserved
    expect(Object.keys(state.rawFiles)).toHaveLength(6);
  });

  it('auto-selects the most recent run report', () => {
    const files = loadAllFixtures();
    useDataStore.getState().loadFiles(files);
    const state = useDataStore.getState();

    expect(state.selectedRunFile).toBe('run-report-sample.md');
  });

  it('selects latest when multiple run reports exist', () => {
    const files = loadAllFixtures();
    // Add a second run report with a "later" filename
    files['run-report-zzz-latest.md'] = files['run-report-sample.md']!;

    useDataStore.getState().loadFiles(files);
    const state = useDataStore.getState();

    // zzz-latest sorts after sample alphabetically
    expect(state.selectedRunFile).toBe('run-report-zzz-latest.md');
    expect(Object.keys(state.runReports)).toHaveLength(2);
  });

  it('shows all available runs in runReports', () => {
    const files = loadAllFixtures();
    files['run-report-alpha.md'] = files['run-report-sample.md']!;
    files['run-report-beta.md'] = files['run-report-sample.md']!;

    useDataStore.getState().loadFiles(files);
    const state = useDataStore.getState();

    const reportFiles = Object.keys(state.runReports).sort();
    expect(reportFiles).toEqual([
      'run-report-alpha.md',
      'run-report-beta.md',
      'run-report-sample.md',
    ]);
  });

  it('allows switching selected run', () => {
    const files = loadAllFixtures();
    files['run-report-other.md'] = files['run-report-sample.md']!;

    useDataStore.getState().loadFiles(files);
    useDataStore.getState().selectRun('run-report-other.md');
    const state = useDataStore.getState();

    expect(state.selectedRunFile).toBe('run-report-other.md');
  });

  it('reset clears all data', () => {
    const files = loadAllFixtures();
    useDataStore.getState().loadFiles(files);
    useDataStore.getState().reset();
    const state = useDataStore.getState();

    expect(state.isLoaded).toBe(false);
    expect(state.selectedRunFile).toBeNull();
    expect(Object.keys(state.runReports)).toHaveLength(0);
    expect(state.iterationDetails).toHaveLength(0);
    expect(state.tsvEntries).toHaveLength(0);
    expect(state.runMetrics).toBeNull();
    expect(state.rules).toHaveLength(0);
  });

  it('handles loading files with no run report', () => {
    useDataStore.getState().loadFiles({
      'iterations.tsv': loadFixture('iterations-sample.tsv'),
    });
    const state = useDataStore.getState();

    expect(state.isLoaded).toBe(true);
    expect(state.selectedRunFile).toBeNull();
    expect(state.tsvEntries).toHaveLength(10);
  });
});

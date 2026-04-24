import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRunReport } from '../../src/parsers/runReport';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('parseRunReport', () => {
  const fixture = loadFixture('run-report-sample.md');
  const report = parseRunReport(fixture);

  it('extracts Run Summary fields', () => {
    expect(report.contextFiles).toBe('(none)');
    expect(report.iterations).toBe(25);
    expect(report.duration).toBe('49m 11s');
    expect(report.commits).toBe(3);
    expect(report.iterationsPerCommit).toBeCloseTo(8.3);
    expect(report.completion).toContain('Done');
  });

  it('extracts Metric Guard fields', () => {
    expect(report.baselinesEstablished).toBe(0);
    expect(report.keeps).toBe(0);
    expect(report.reverts).toBe(0);
    expect(report.testsPassed).toBe(0);
    expect(report.testsFailed).toBe(0);
  });

  it('extracts Agent Behavior fields', () => {
    expect(report.stuckEvents).toBe(0);
    expect(report.checkpointRefreshes).toBe(1);
    expect(report.verifyKeeps).toBe(0);
    expect(report.verifyReverts).toBe(0);
  });

  it('extracts Trigger Sources with bold markers', () => {
    expect(report.triggerPrompt).toBe(0);
    expect(report.triggerShellPrompt).toBe(0);
    expect(report.triggerFast).toBe(17);
    expect(report.triggerSlow).toBe(30);
    expect(report.triggerTotal).toBe(47);
  });

  it('extracts commit log from code block', () => {
    expect(report.commitLog).toHaveLength(3);
    expect(report.commitLog[0]).toContain('de955c3');
    expect(report.commitLog[2]).toContain('9db03bb');
  });

  it('separates feedback markdown', () => {
    expect(report.feedbackMarkdown).toContain('# Feedback Loop Summary');
    expect(report.feedbackMarkdown).toContain('Tier 1');
    expect(report.feedbackMarkdown).toContain('Tier 2');
  });

  it('handles report with 0 commits and 0 stuck events', () => {
    const minimal = `# Orchestrator Post-Mortem Report

## Run Summary

| Metric | Value |
|--------|-------|
| Context Files | (none) |
| Completion | manual stop |
| Iterations | 5 |
| Duration | 2m 30s |
| Commits | 0 |
| Iterations/commit | 0 |

## Metric Guard

| Metric | Value |
|--------|-------|
| Baselines established | 0 |
| Keeps (changes passed) | 0 |
| Reverts (regressions caught) | 0 |
| Final test count | 0 passed, 0 failed |

## Agent Behavior

| Metric | Value |
|--------|-------|
| Stuck events | 0 |
| Checkpoint refreshes | 0 |
| Verify keeps (from TSV) | 0 |
| Verify reverts (from TSV) | 0 |

## Commits

\`\`\`
\`\`\`
`;
    const r = parseRunReport(minimal);
    expect(r.commits).toBe(0);
    expect(r.stuckEvents).toBe(0);
    expect(r.commitLog).toHaveLength(0);
    expect(r.iterations).toBe(5);
  });

  it('handles missing sections gracefully', () => {
    const partial = `# Orchestrator Post-Mortem Report

## Run Summary

| Metric | Value |
|--------|-------|
| Iterations | 10 |
| Duration | 5m 0s |
| Commits | 1 |
| Iterations/commit | 10 |
`;
    const r = parseRunReport(partial);
    expect(r.iterations).toBe(10);
    expect(r.commits).toBe(1);
    // Missing sections default to 0
    expect(r.stuckEvents).toBe(0);
    expect(r.triggerTotal).toBe(0);
    expect(r.feedbackMarkdown).toBe('');
  });
});

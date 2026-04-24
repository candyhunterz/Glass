import { describe, it, expect } from 'vitest';
import {
  efficiencyScore,
  stallRate,
  responseRatio,
  waitRatio,
  avgIterationDuration,
  parseDurationToSecs,
} from '../../src/utils/analytics';
import type { RunReport, IterationDetailEntry, TsvEntry } from '../../src/types';

function makeReport(overrides: Partial<RunReport>): RunReport {
  return {
    contextFiles: '',
    completion: '',
    iterations: 0,
    duration: '0s',
    commits: 0,
    iterationsPerCommit: 0,
    baselinesEstablished: 0,
    keeps: 0,
    reverts: 0,
    testsPassed: 0,
    testsFailed: 0,
    stuckEvents: 0,
    checkpointRefreshes: 0,
    verifyKeeps: 0,
    verifyReverts: 0,
    triggerPrompt: 0,
    triggerShellPrompt: 0,
    triggerFast: 0,
    triggerSlow: 0,
    triggerTotal: 0,
    commitLog: [],
    feedbackMarkdown: '',
    ...overrides,
  };
}

function makeEntry(overrides: Partial<IterationDetailEntry>): IterationDetailEntry {
  return {
    iteration: 1,
    timestamp: '00:00:00',
    triggerSource: null,
    action: null,
    instruction: null,
    filesChanged: [],
    verifyResult: null,
    error: null,
    note: null,
    silenceDurationMs: null,
    blockExecuting: false,
    ...overrides,
  };
}

function makeTsv(overrides: Partial<TsvEntry>): TsvEntry {
  return {
    iteration: 1,
    commit: '',
    feature: '',
    metric: '',
    status: 'instruction',
    description: '',
    ...overrides,
  };
}

describe('efficiencyScore', () => {
  it('computes correctly with normal values', () => {
    const report = makeReport({ commits: 3, iterations: 25, stuckEvents: 0, reverts: 0 });
    expect(efficiencyScore(report)).toBeCloseTo(1.2);
  });

  it('higher commits produce higher score', () => {
    const report = makeReport({ commits: 10, iterations: 20, stuckEvents: 0, reverts: 0 });
    expect(efficiencyScore(report)).toBe(5);
  });

  it('stuck events and reverts lower the score', () => {
    const report = makeReport({ commits: 3, iterations: 20, stuckEvents: 5, reverts: 5 });
    expect(efficiencyScore(report)).toBe(1); // 30 / 30
  });

  it('returns 0 when iterations is 0', () => {
    const report = makeReport({ commits: 0, iterations: 0, stuckEvents: 0, reverts: 0 });
    expect(efficiencyScore(report)).toBe(0);
  });

  it('returns 0 when commits is 0', () => {
    const report = makeReport({ commits: 0, iterations: 10, stuckEvents: 0, reverts: 0 });
    expect(efficiencyScore(report)).toBe(0);
  });
});

describe('stallRate', () => {
  it('returns 0 for empty entries', () => {
    expect(stallRate([])).toBe(0);
  });

  it('returns 0 when no stalls', () => {
    const entries = [
      makeEntry({ blockExecuting: false }),
      makeEntry({ blockExecuting: false }),
    ];
    expect(stallRate(entries)).toBe(0);
  });

  it('computes correct stall percentage', () => {
    const entries = [
      makeEntry({ blockExecuting: true }),
      makeEntry({ blockExecuting: false }),
      makeEntry({ blockExecuting: true }),
      makeEntry({ blockExecuting: false }),
    ];
    expect(stallRate(entries)).toBe(0.5);
  });

  it('returns 1 when all iterations are stalls', () => {
    const entries = [
      makeEntry({ blockExecuting: true }),
      makeEntry({ blockExecuting: true }),
    ];
    expect(stallRate(entries)).toBe(1);
  });
});

describe('responseRatio', () => {
  it('returns 0 when triggerTotal is 0', () => {
    expect(responseRatio([], 0)).toBe(0);
  });

  it('counts instruction, checkpoint, and done as responses', () => {
    const entries = [
      makeEntry({ action: 'instruction' }),
      makeEntry({ action: 'wait' }),
      makeEntry({ action: 'checkpoint' }),
      makeEntry({ action: 'done' }),
      makeEntry({ action: 'stuck' }),
    ];
    // 3 responses out of 10 triggers
    expect(responseRatio(entries, 10)).toBeCloseTo(0.3);
  });
});

describe('waitRatio', () => {
  it('returns 0 for empty entries', () => {
    expect(waitRatio([])).toBe(0);
  });

  it('computes correct wait ratio', () => {
    const entries = [
      makeTsv({ status: 'instruction' }),
      makeTsv({ status: 'wait' }),
      makeTsv({ status: 'instruction' }),
      makeTsv({ status: 'wait' }),
      makeTsv({ status: 'stuck' }),
    ];
    expect(waitRatio(entries)).toBeCloseTo(0.4);
  });

  it('returns 0 when no waits', () => {
    const entries = [
      makeTsv({ status: 'instruction' }),
      makeTsv({ status: 'checkpoint' }),
    ];
    expect(waitRatio(entries)).toBe(0);
  });
});

describe('avgIterationDuration', () => {
  it('computes average', () => {
    expect(avgIterationDuration(300, 10)).toBe(30);
  });

  it('returns 0 when iterations is 0', () => {
    expect(avgIterationDuration(300, 0)).toBe(0);
  });
});

describe('parseDurationToSecs', () => {
  it('parses minutes and seconds', () => {
    expect(parseDurationToSecs('49m 11s')).toBe(2951);
  });

  it('parses seconds only', () => {
    expect(parseDurationToSecs('45s')).toBe(45);
  });

  it('parses minutes only', () => {
    expect(parseDurationToSecs('5m')).toBe(300);
  });

  it('handles empty string', () => {
    expect(parseDurationToSecs('')).toBe(0);
  });
});

import type { RunReport, IterationDetailEntry, TsvEntry } from '../types';

/**
 * Efficiency score: (commits x 10) / (iterations + stuck_events + reverts).
 * Higher is better. Returns 0 if denominator is 0.
 */
export function efficiencyScore(report: RunReport): number {
  const denom = report.iterations + report.stuckEvents + report.reverts;
  return denom > 0 ? (report.commits * 10) / denom : 0;
}

/**
 * Stall rate: percentage of iterations with [BLOCK STILL EXECUTING].
 * Returns 0-1 ratio.
 */
export function stallRate(entries: IterationDetailEntry[]): number {
  if (entries.length === 0) return 0;
  const stalls = entries.filter((e) => e.blockExecuting).length;
  return stalls / entries.length;
}

/**
 * Response ratio: iterations with actual responses / total trigger count.
 * A "response" is any iteration with action instruction, checkpoint, or done.
 */
export function responseRatio(
  entries: IterationDetailEntry[],
  triggerTotal: number,
): number {
  if (triggerTotal === 0) return 0;
  const responses = entries.filter(
    (e) =>
      e.action === 'instruction' ||
      e.action === 'checkpoint' ||
      e.action === 'done',
  ).length;
  return responses / triggerTotal;
}

/**
 * Wait ratio: wait iterations / total iterations.
 */
export function waitRatio(tsvEntries: TsvEntry[]): number {
  if (tsvEntries.length === 0) return 0;
  const waits = tsvEntries.filter((e) => e.status === 'wait').length;
  return waits / tsvEntries.length;
}

/**
 * Average iteration duration in seconds.
 */
export function avgIterationDuration(
  durationSecs: number,
  iterations: number,
): number {
  return iterations > 0 ? durationSecs / iterations : 0;
}

/**
 * Parse a duration string like "49m 11s" into seconds.
 */
export function parseDurationToSecs(duration: string): number {
  const minMatch = duration.match(/(\d+)m/);
  const secMatch = duration.match(/(\d+)s/);
  return (
    (minMatch ? parseInt(minMatch[1]!, 10) * 60 : 0) +
    (secMatch ? parseInt(secMatch[1]!, 10) : 0)
  );
}

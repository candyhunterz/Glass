import { parse } from 'smol-toml';
import type { RunMetrics, RunMetricsEntry, RuleFiring } from '../types';

/**
 * Parse run-metrics.toml into a RunMetrics object.
 */
export function parseRunMetrics(toml: string): RunMetrics {
  const data = parse(toml) as Record<string, unknown>;
  const rawRuns = (data['runs'] as unknown[]) ?? [];

  const runs: RunMetricsEntry[] = rawRuns.map((raw) => {
    const r = raw as Record<string, unknown>;
    const rawFireings = (r['rule_firings'] as unknown[]) ?? [];

    const rule_firings: RuleFiring[] = rawFireings.map((rf) => {
      const f = rf as Record<string, unknown>;
      return {
        rule_id: String(f['rule_id'] ?? ''),
        action: String(f['action'] ?? ''),
        firing_count: Number(f['firing_count'] ?? 0),
      };
    });

    return {
      run_id: String(r['run_id'] ?? ''),
      project_root: String(r['project_root'] ?? ''),
      iterations: Number(r['iterations'] ?? 0),
      duration_secs: Number(r['duration_secs'] ?? 0),
      revert_rate: Number(r['revert_rate'] ?? 0),
      stuck_rate: Number(r['stuck_rate'] ?? 0),
      waste_rate: Number(r['waste_rate'] ?? 0),
      checkpoint_rate: Number(r['checkpoint_rate'] ?? 0),
      completion: String(r['completion'] ?? ''),
      prd_items_completed: Number(r['prd_items_completed'] ?? 0),
      prd_items_total: Number(r['prd_items_total'] ?? 0),
      kickoff_duration_secs: Number(r['kickoff_duration_secs'] ?? 0),
      rule_firings,
    };
  });

  return { runs };
}

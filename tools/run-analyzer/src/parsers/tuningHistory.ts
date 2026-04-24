import { parse } from 'smol-toml';
import type { TuningHistory, TuningSnapshot, PendingChange, Cooldown } from '../types';

/**
 * Parse tuning-history.toml into a TuningHistory object.
 */
export function parseTuningHistory(toml: string): TuningHistory {
  const data = parse(toml) as Record<string, unknown>;

  const rawSnapshots = (data['snapshots'] as unknown[]) ?? [];
  const snapshots: TuningSnapshot[] = rawSnapshots.map((raw) => {
    const s = raw as Record<string, unknown>;
    return {
      run_id: String(s['run_id'] ?? ''),
      provisional_rules: Array.isArray(s['provisional_rules'])
        ? (s['provisional_rules'] as string[])
        : [],
      config_values: (s['config_values'] as Record<string, string>) ?? {},
    };
  });

  const rawPending = data['pending'] as Record<string, unknown> | undefined;
  const pending: PendingChange | null = rawPending
    ? {
        field: String(rawPending['field'] ?? ''),
        old_value: String(rawPending['old_value'] ?? ''),
        new_value: String(rawPending['new_value'] ?? ''),
        finding_id: String(rawPending['finding_id'] ?? ''),
        run_id: String(rawPending['run_id'] ?? ''),
      }
    : null;

  const rawCooldowns = (data['cooldowns'] as unknown[]) ?? [];
  const cooldowns: Cooldown[] = rawCooldowns.map((raw) => {
    const c = raw as Record<string, unknown>;
    return {
      field: String(c['field'] ?? ''),
      remaining_runs: Number(c['remaining_runs'] ?? 0),
    };
  });

  return { snapshots, pending, cooldowns };
}

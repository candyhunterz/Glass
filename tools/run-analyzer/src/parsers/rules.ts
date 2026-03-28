import { parse } from 'smol-toml';
import type { Rule } from '../types';

function mapRule(raw: unknown): Rule {
  const r = raw as Record<string, unknown>;
  return {
    id: String(r['id'] ?? ''),
    trigger: String(r['trigger'] ?? ''),
    action: String(r['action'] ?? ''),
    status: (String(r['status'] ?? 'provisional') as Rule['status']),
    severity: (String(r['severity'] ?? 'low') as Rule['severity']),
    scope: (String(r['scope'] ?? 'project') as Rule['scope']),
    tags: Array.isArray(r['tags']) ? (r['tags'] as string[]) : [],
    added_run: String(r['added_run'] ?? ''),
    confirmed_run: String(r['confirmed_run'] ?? ''),
    rejected_run: String(r['rejected_run'] ?? ''),
    rejected_reason: String(r['rejected_reason'] ?? ''),
    trigger_count: Number(r['trigger_count'] ?? 0),
    cooldown_remaining: Number(r['cooldown_remaining'] ?? 0),
    stale_runs: Number(r['stale_runs'] ?? 0),
    ablation_result: String(r['ablation_result'] ?? ''),
  };
}

/**
 * Parse rules.toml into an array of Rule objects.
 */
export function parseRules(toml: string): Rule[] {
  const data = parse(toml) as Record<string, unknown>;
  const rawRules = (data['rules'] as unknown[]) ?? [];
  return rawRules.map(mapRule);
}

/**
 * Parse archived-rules.toml into an array of Rule objects.
 * Same format as rules.toml — rules may be empty.
 */
export function parseArchivedRules(toml: string): Rule[] {
  const data = parse(toml) as Record<string, unknown>;
  const rawRules = (data['rules'] as unknown[]) ?? [];
  return rawRules.map(mapRule);
}

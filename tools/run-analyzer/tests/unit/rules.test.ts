import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRules, parseArchivedRules } from '../../src/parsers/rules';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('parseRules', () => {
  const fixture = loadFixture('rules-sample.toml');
  const rules = parseRules(fixture);

  it('parses multiple rules', () => {
    expect(rules).toHaveLength(2);
  });

  it('extracts rule fields', () => {
    const rule = rules[0]!;
    expect(rule.id).toBe('uncommitted-drift');
    expect(rule.trigger).toBe('behavioral');
    expect(rule.action).toBe('force_commit');
    expect(rule.status).toBe('provisional');
    expect(rule.severity).toBe('medium');
    expect(rule.scope).toBe('global');
    expect(rule.added_run).toBe('run-1774683967');
  });

  it('extracts second rule', () => {
    const rule = rules[1]!;
    expect(rule.id).toBe('oscillation');
    expect(rule.action).toBe('early_stuck');
  });

  it('handles tags array', () => {
    expect(rules[0]!.tags).toEqual([]);
  });

  it('handles numeric fields', () => {
    expect(rules[0]!.trigger_count).toBe(0);
    expect(rules[0]!.cooldown_remaining).toBe(0);
    expect(rules[0]!.stale_runs).toBe(0);
  });

  it('handles ablation result', () => {
    expect(rules[0]!.ablation_result).toBe('untested');
  });
});

describe('parseArchivedRules', () => {
  it('handles empty rules file', () => {
    const emptyToml = `rules = []

[meta]
version = ""
description = ""
`;
    const rules = parseArchivedRules(emptyToml);
    expect(rules).toHaveLength(0);
  });

  it('parses archived rules with rejection info', () => {
    const toml = `[meta]
version = ""
description = ""

[[rules]]
id = "old-rule"
trigger = "behavioral"
action = "force_commit"
status = "rejected"
severity = "medium"
scope = "global"
tags = []
added_run = "run-100"
added_metric = ""
confirmed_run = ""
rejected_run = "run-200"
rejected_reason = "regression detected"
last_triggered_run = ""
trigger_count = 5
cooldown_remaining = 0
stale_runs = 3
last_ablation_run = ""
ablation_result = "negative"

[rules.trigger_params]

[rules.action_params]
`;
    const rules = parseArchivedRules(toml);
    expect(rules).toHaveLength(1);
    expect(rules[0]!.id).toBe('old-rule');
    expect(rules[0]!.status).toBe('rejected');
    expect(rules[0]!.rejected_run).toBe('run-200');
    expect(rules[0]!.rejected_reason).toBe('regression detected');
    expect(rules[0]!.trigger_count).toBe(5);
    expect(rules[0]!.stale_runs).toBe(3);
    expect(rules[0]!.ablation_result).toBe('negative');
  });
});

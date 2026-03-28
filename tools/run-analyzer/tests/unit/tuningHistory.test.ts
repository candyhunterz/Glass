import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseTuningHistory } from '../../src/parsers/tuningHistory';

const fixturesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../fixtures');

function loadFixture(name: string): string {
  return readFileSync(resolve(fixturesDir, name), 'utf-8');
}

describe('parseTuningHistory', () => {
  const fixture = loadFixture('tuning-history-sample.toml');
  const history = parseTuningHistory(fixture);

  it('parses snapshots', () => {
    expect(history.snapshots).toHaveLength(1);
    expect(history.snapshots[0]!.run_id).toBe('run-1774683967');
  });

  it('extracts config values from snapshot', () => {
    const config = history.snapshots[0]!.config_values;
    expect(config['feedback_llm']).toBe('true');
    expect(config['max_prompt_hints']).toBe('10');
    expect(config['silence_timeout_secs']).toBe('5');
    expect(config['max_retries_before_stuck']).toBe('4');
  });

  it('extracts pending change', () => {
    expect(history.pending).not.toBeNull();
    expect(history.pending!.field).toBe('max_retries_before_stuck');
    expect(history.pending!.old_value).toBe('4');
    expect(history.pending!.new_value).toBe('5');
    expect(history.pending!.finding_id).toBe('stuck-sensitivity');
    expect(history.pending!.run_id).toBe('run-1774683967');
  });

  it('parses empty cooldowns', () => {
    expect(history.cooldowns).toHaveLength(0);
  });

  it('handles file with no pending changes', () => {
    const noPending = `cooldowns = []

[[snapshots]]
run_id = "run-abc"
provisional_rules = ["rule-1"]

[snapshots.config_values]
feedback_llm = "false"
`;
    const result = parseTuningHistory(noPending);
    expect(result.pending).toBeNull();
    expect(result.snapshots).toHaveLength(1);
    expect(result.snapshots[0]!.provisional_rules).toEqual(['rule-1']);
  });

  it('handles cooldowns array', () => {
    const withCooldowns = `[[cooldowns]]
field = "silence_timeout_secs"
remaining_runs = 3

[[cooldowns]]
field = "max_retries_before_stuck"
remaining_runs = 1

[[snapshots]]
run_id = "run-xyz"
provisional_rules = []

[snapshots.config_values]
feedback_llm = "true"
`;
    const result = parseTuningHistory(withCooldowns);
    expect(result.cooldowns).toHaveLength(2);
    expect(result.cooldowns[0]!.field).toBe('silence_timeout_secs');
    expect(result.cooldowns[0]!.remaining_runs).toBe(3);
    expect(result.cooldowns[1]!.remaining_runs).toBe(1);
  });
});

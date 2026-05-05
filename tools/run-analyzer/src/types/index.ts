/** Parsed from run-report-*.md */
export interface RunReport {
  // Run Summary table
  contextFiles: string;
  completion: string;
  iterations: number;
  duration: string;
  commits: number;
  iterationsPerCommit: number;

  // Metric Guard table
  baselinesEstablished: number;
  keeps: number;
  reverts: number;
  testsPassed: number;
  testsFailed: number;

  // Agent Behavior table
  stuckEvents: number;
  checkpointRefreshes: number;
  verifyKeeps: number;
  verifyReverts: number;

  // Trigger Sources table
  triggerPrompt: number;
  triggerShellPrompt: number;
  triggerFast: number;
  triggerSlow: number;
  triggerTotal: number;

  // Commits section
  commitLog: string[];

  // Feedback sections (raw markdown for rendering)
  feedbackMarkdown: string;
}

/** Parsed from iteration-details.md — one entry per iteration */
export interface IterationDetailEntry {
  iteration: number;
  timestamp: string;
  triggerSource: string | null;
  action: string | null;
  instruction: string | null;
  filesChanged: string[];
  verifyResult: string | null;
  error: string | null;
  note: string | null;

  // Derived
  silenceDurationMs: number | null;
  blockExecuting: boolean;
}

/** Parsed from iterations.tsv — one row per logged iteration */
export interface TsvEntry {
  iteration: number;
  commit: string;
  feature: string;
  metric: string;
  status: string;
  description: string;
}

/** A single rule firing record nested inside RunMetricsEntry */
export interface RuleFiring {
  rule_id: string;
  action: string;
  firing_count: number;
}

/** A single run record from run-metrics.toml */
export interface RunMetricsEntry {
  run_id: string;
  project_root: string;
  iterations: number;
  duration_secs: number;
  revert_rate: number;
  stuck_rate: number;
  waste_rate: number;
  checkpoint_rate: number;
  completion: string;
  prd_items_completed: number;
  prd_items_total: number;
  kickoff_duration_secs: number;
  rule_firings: RuleFiring[];
}

/** Parsed from run-metrics.toml */
export interface RunMetrics {
  runs: RunMetricsEntry[];
}

/** Parsed from rules.toml or archived-rules.toml */
export interface Rule {
  id: string;
  trigger: string;
  action: string;
  status: 'provisional' | 'confirmed' | 'rejected';
  severity: 'low' | 'medium' | 'high';
  scope: 'project' | 'global';
  tags: string[];
  added_run: string;
  confirmed_run: string;
  rejected_run: string;
  rejected_reason: string;
  trigger_count: number;
  cooldown_remaining: number;
  stale_runs: number;
  ablation_result: string;
}

/** A config snapshot from tuning-history.toml */
export interface TuningSnapshot {
  run_id: string;
  provisional_rules: string[];
  config_values: Record<string, string>;
}

/** A pending config change from tuning-history.toml */
export interface PendingChange {
  field: string;
  old_value: string;
  new_value: string;
  finding_id: string;
  run_id: string;
}

/** A config field cooldown from tuning-history.toml */
export interface Cooldown {
  field: string;
  remaining_runs: number;
}

/** Parsed from tuning-history.toml */
export interface TuningHistory {
  snapshots: TuningSnapshot[];
  pending: PendingChange | null;
  cooldowns: Cooldown[];
}

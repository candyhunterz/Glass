/** Status colors matching the Glass overlay palette */
export const STATUS_COLORS: Record<string, string> = {
  instruction: '#8CA0B4',
  wait: '#64646E',
  stuck: '#DCA03C',
  checkpoint: '#6478FF',
  keep: '#509678',
  baseline: '#509678',
  revert: '#FF5050',
  complete: '#B48CFF',
  done: '#B48CFF',
};

/** Trigger source colors for charts */
export const TRIGGER_COLORS: Record<string, string> = {
  Prompt: '#509678',
  ShellPrompt: '#6478FF',
  Fast: '#DCA03C',
  Slow: '#64646E',
};

export function getStatusColor(status: string): string {
  return STATUS_COLORS[status] ?? '#8CA0B4';
}

export function getTriggerColor(source: string): string {
  const type = source.split(',')[0]?.trim() ?? source;
  return TRIGGER_COLORS[type] ?? '#64646E';
}

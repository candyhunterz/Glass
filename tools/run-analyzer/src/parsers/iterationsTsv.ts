import type { TsvEntry } from '../types';

/**
 * Parse iterations.tsv into an array of TsvEntry.
 *
 * Handles:
 * - Header row (skipped)
 * - Run separator rows starting with `---` or `# Run:`
 * - Empty/missing columns
 */
export function parseIterationsTsv(tsv: string): TsvEntry[] {
  const lines = tsv.split('\n');
  const entries: TsvEntry[] = [];
  let headerSkipped = false;

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (line.length === 0) continue;

    // Skip run separator rows
    if (line.startsWith('---') || line.startsWith('# Run:')) continue;

    // Skip header row
    if (!headerSkipped && line.startsWith('iteration')) {
      headerSkipped = true;
      continue;
    }

    const cols = line.split('\t');
    const iterationStr = cols[0] ?? '';
    const iteration = parseInt(iterationStr, 10);
    if (isNaN(iteration)) continue;

    entries.push({
      iteration,
      commit: cols[1] ?? '',
      feature: cols[2] ?? '',
      metric: cols[3] ?? '',
      status: cols[4] ?? '',
      description: cols[5] ?? '',
    });
  }

  return entries;
}

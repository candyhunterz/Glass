import type { RunReport } from '../types';

/**
 * Strip markdown bold markers from a string.
 */
function stripBold(s: string): string {
  return s.replace(/\*\*/g, '');
}

/**
 * Parse a markdown table section into a key→value map.
 * Uses line-by-line state machine per PRD requirements.
 */
function parseMarkdownTable(lines: string[]): Map<string, string> {
  const result = new Map<string, string>();
  let inTable = false;
  let headerSeen = false;

  for (const line of lines) {
    const trimmed = line.trim();

    if (!trimmed.startsWith('|')) {
      if (inTable) break; // table ended
      continue;
    }

    // Split cells: split on | then filter empty edge entries
    const cells = trimmed
      .split('|')
      .map((c) => c.trim());

    // Remove empty strings from leading/trailing pipes
    const filtered = cells.filter((c) => c.length > 0);
    if (filtered.length < 2) continue;

    // Skip separator row (---+---)
    if (filtered.every((c) => /^[-:]+$/.test(c))) {
      inTable = true;
      headerSeen = true;
      continue;
    }

    // Skip the header row (first row before separator)
    if (!headerSeen) {
      continue;
    }

    const key = stripBold(filtered[0]!).trim();
    const value = stripBold(filtered[1]!).trim();
    result.set(key, value);
  }

  return result;
}

interface Section {
  title: string;
  lines: string[];
}

/**
 * Split markdown into sections by `## ` headers.
 * Stops at `# ` top-level headers (feedback boundary).
 */
function splitSections(markdown: string): Section[] {
  const sections: Section[] = [];
  const allLines = markdown.split('\n');
  let currentTitle = '';
  let currentLines: string[] = [];

  for (const line of allLines) {
    if (line.startsWith('## ')) {
      if (currentTitle) {
        sections.push({ title: currentTitle, lines: currentLines });
      }
      currentTitle = line.slice(3).trim();
      currentLines = [];
    } else {
      currentLines.push(line);
    }
  }

  if (currentTitle) {
    sections.push({ title: currentTitle, lines: currentLines });
  }

  return sections;
}

/**
 * Extract commit lines from a fenced code block in a section.
 */
function extractCommitLog(lines: string[]): string[] {
  const commits: string[] = [];
  let inBlock = false;

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith('```')) {
      if (inBlock) break;
      inBlock = true;
      continue;
    }
    if (inBlock && trimmed.length > 0) {
      // Skip TSV header rows in the raw iteration log
      if (trimmed.startsWith('iteration\t')) continue;
      commits.push(trimmed);
    }
  }

  return commits;
}

export function parseRunReport(markdown: string): RunReport {
  // Separate report body from feedback markdown
  const feedbackIdx = markdown.indexOf('# Feedback Loop Summary');
  const reportPart = feedbackIdx >= 0 ? markdown.slice(0, feedbackIdx) : markdown;
  const feedbackMarkdown = feedbackIdx >= 0 ? markdown.slice(feedbackIdx).trim() : '';

  const sections = splitSections(reportPart);

  function getTable(sectionTitle: string): Map<string, string> {
    const section = sections.find((s) => s.title === sectionTitle);
    if (!section) return new Map();
    return parseMarkdownTable(section.lines);
  }

  const summary = getTable('Run Summary');
  const metricGuard = getTable('Metric Guard');
  const agentBehavior = getTable('Agent Behavior');
  const triggerSources = getTable('Trigger Sources');

  // Extract commits from the Commits section code block
  const commitsSection = sections.find((s) => s.title === 'Commits');
  const commitLog = commitsSection ? extractCommitLog(commitsSection.lines) : [];

  // Parse "N passed, M failed" from test count field
  const testCountStr = metricGuard.get('Final test count') ?? '0 passed, 0 failed';
  const passedMatch = testCountStr.match(/(\d+)\s*passed/);
  const failedMatch = testCountStr.match(/(\d+)\s*failed/);

  return {
    contextFiles: summary.get('Context Files') ?? '',
    completion: summary.get('Completion') ?? '',
    iterations: parseInt(summary.get('Iterations') ?? '0', 10),
    duration: summary.get('Duration') ?? '',
    commits: parseInt(summary.get('Commits') ?? '0', 10),
    iterationsPerCommit: parseFloat(summary.get('Iterations/commit') ?? '0'),

    baselinesEstablished: parseInt(metricGuard.get('Baselines established') ?? '0', 10),
    keeps: parseInt(metricGuard.get('Keeps (changes passed)') ?? '0', 10),
    reverts: parseInt(metricGuard.get('Reverts (regressions caught)') ?? '0', 10),
    testsPassed: passedMatch ? parseInt(passedMatch[1]!, 10) : 0,
    testsFailed: failedMatch ? parseInt(failedMatch[1]!, 10) : 0,

    stuckEvents: parseInt(agentBehavior.get('Stuck events') ?? '0', 10),
    checkpointRefreshes: parseInt(agentBehavior.get('Checkpoint refreshes') ?? '0', 10),
    verifyKeeps: parseInt(agentBehavior.get('Verify keeps (from TSV)') ?? '0', 10),
    verifyReverts: parseInt(agentBehavior.get('Verify reverts (from TSV)') ?? '0', 10),

    triggerPrompt: parseInt(
      triggerSources.get('Prompt regex') ?? triggerSources.get('Prompt') ?? '0',
      10,
    ),
    triggerShellPrompt: parseInt(
      triggerSources.get('Shell prompt') ?? triggerSources.get('ShellPrompt') ?? '0',
      10,
    ),
    triggerFast: parseInt(
      triggerSources.get('Fast (velocity)') ?? triggerSources.get('Fast') ?? '0',
      10,
    ),
    triggerSlow: parseInt(
      triggerSources.get('Slow (fallback)') ?? triggerSources.get('Slow') ?? '0',
      10,
    ),
    triggerTotal: parseInt(triggerSources.get('Total') ?? '0', 10),

    commitLog,
    feedbackMarkdown,
  };
}

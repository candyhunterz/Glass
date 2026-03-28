import type { IterationDetailEntry } from '../types';

/**
 * Extract a `**FieldName:** value` field from a block of markdown text.
 * Returns null if the field is missing or its value is empty.
 */
function extractField(block: string, fieldName: string): string | null {
  const regex = new RegExp(`\\*\\*${fieldName}:\\*\\*[ \\t]*(.*)$`, 'm');
  const match = block.match(regex);
  if (!match) return null;
  const value = match[1]!.trim();
  return value.length > 0 ? value : null;
}

/**
 * Parse a single iteration block into an IterationDetailEntry.
 */
function parseIterationBlock(
  block: string,
  iteration: number,
  timestamp: string,
): IterationDetailEntry {
  const trigger = extractField(block, 'Trigger');
  const action = extractField(block, 'Action');
  const instruction = extractField(block, 'Agent instruction');
  const verifyResult = extractField(block, 'Verify result') ?? extractField(block, 'Result');
  const error = extractField(block, 'Error');
  const note = extractField(block, 'Note');

  // Extract files changed — comma-separated inline or bullet list
  const filesChanged: string[] = [];
  const inlineFiles = extractField(block, 'Files changed');
  if (inlineFiles) {
    filesChanged.push(
      ...inlineFiles
        .split(',')
        .map((f) => f.trim())
        .filter(Boolean),
    );
  }
  // Also handle bullet-list format after the field header
  const bulletListRegex = /\*\*Files changed:\*\*\s*\n((?:\s*[-*]\s+.+\n?)+)/m;
  const bulletMatch = block.match(bulletListRegex);
  if (bulletMatch) {
    const items = bulletMatch[1]!.match(/[-*]\s+(.+)/g);
    if (items) {
      filesChanged.push(...items.map((item) => item.replace(/^[-*]\s+/, '').trim()));
    }
  }

  // Derive silence duration and block-executing flag from trigger source
  let silenceDurationMs: number | null = null;
  let blockExecuting = false;

  if (trigger) {
    const silenceMatch = trigger.match(/silence=(\d+)ms/);
    if (silenceMatch) {
      silenceDurationMs = parseInt(silenceMatch[1]!, 10);
    }
    blockExecuting = trigger.includes('[BLOCK STILL EXECUTING]');
  }

  return {
    iteration,
    timestamp,
    triggerSource: trigger,
    action,
    instruction,
    filesChanged,
    verifyResult,
    error,
    note,
    silenceDurationMs,
    blockExecuting,
  };
}

/**
 * Parse iteration-details.md into an array of IterationDetailEntry.
 *
 * Handles:
 * - `## Iteration N [HH:MM:SS]` headers
 * - Run separator lines (`---`, `# Run:`)
 * - Missing fields within entries
 * - Non-sequential iteration numbers
 */
export function parseIterationDetails(markdown: string): IterationDetailEntry[] {
  const entries: IterationDetailEntry[] = [];

  // Find all iteration headers with their positions
  const headerRegex = /^## Iteration (\d+) \[(\d{2}:\d{2}:\d{2})\]/gm;
  const headers: { index: number; iteration: number; timestamp: string }[] = [];

  let match;
  while ((match = headerRegex.exec(markdown)) !== null) {
    headers.push({
      index: match.index,
      iteration: parseInt(match[1]!, 10),
      timestamp: match[2]!,
    });
  }

  for (let i = 0; i < headers.length; i++) {
    const start = headers[i]!.index;
    const end = i + 1 < headers.length ? headers[i + 1]!.index : markdown.length;
    const block = markdown.slice(start, end);
    entries.push(parseIterationBlock(block, headers[i]!.iteration, headers[i]!.timestamp));
  }

  return entries;
}

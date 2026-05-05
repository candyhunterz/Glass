import { useRef, useEffect, useState } from 'react';
import * as d3 from 'd3';
import type { IterationDetailEntry } from '../../types';
import { getStatusColor } from '../../utils/colors';

interface Props {
  entries: IterationDetailEntry[];
  selectedIteration: number | null;
  onSelectIteration: (iteration: number | null) => void;
}

interface TooltipState {
  x: number;
  y: number;
  lines: string[];
}

function parseTimestamp(ts: string): number {
  const parts = ts.split(':').map(Number);
  return (parts[0] ?? 0) * 3600 + (parts[1] ?? 0) * 60 + (parts[2] ?? 0);
}

export function IterationTimeline({
  entries,
  selectedIteration,
  onSelectIteration,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  useEffect(() => {
    if (!svgRef.current || entries.length === 0) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const margin = { top: 10, right: 20, bottom: 20, left: 55 };
    const width = 750;
    const rowHeight = 26;
    const height = entries.length * rowHeight;

    svg
      .attr('width', width + margin.left + margin.right)
      .attr('height', height + margin.top + margin.bottom);

    const g = svg
      .append('g')
      .attr('transform', `translate(${margin.left},${margin.top})`);

    // Time computation
    const timestamps = entries.map((e) => parseTimestamp(e.timestamp));
    const startTime = timestamps[0] ?? 0;
    const endTime = timestamps[timestamps.length - 1] ?? startTime;
    const totalDuration = Math.max(endTime - startTime, 1);

    const xScale = d3.scaleLinear().domain([0, totalDuration]).range([0, width]);
    const yScale = d3
      .scaleBand<number>()
      .domain(entries.map((e) => e.iteration))
      .range([0, height])
      .padding(0.2);

    const container = containerRef.current;

    // Iteration blocks with hover + click
    g.selectAll<SVGRectElement, IterationDetailEntry>('rect.block')
      .data(entries)
      .join('rect')
      .attr('class', 'block')
      .attr('x', (_d, i) => xScale((timestamps[i] ?? 0) - startTime))
      .attr('y', (d) => yScale(d.iteration) ?? 0)
      .attr('width', (_d, i) => {
        const start = (timestamps[i] ?? 0) - startTime;
        const end =
          i + 1 < timestamps.length
            ? (timestamps[i + 1] ?? 0) - startTime
            : totalDuration;
        return Math.max(xScale(end) - xScale(start), 6);
      })
      .attr('height', yScale.bandwidth())
      .attr('fill', (d) => getStatusColor(d.action ?? 'instruction'))
      .attr('rx', 2)
      .attr('cursor', 'pointer')
      .attr('opacity', (d) =>
        selectedIteration !== null && d.iteration !== selectedIteration
          ? 0.4
          : 1,
      )
      .on('click', (_event, d) => onSelectIteration(d.iteration))
      .on('mouseover', function (event: MouseEvent, d) {
        d3.select(this).attr('stroke', '#e5e7eb').attr('stroke-width', 1.5);
        if (!container) return;
        const [mx, my] = d3.pointer(event, container);
        const lines = [`#${d.iteration}  [${d.timestamp}]`, `Action: ${d.action ?? 'unknown'}`];
        if (d.triggerSource) lines.push(`Trigger: ${d.triggerSource.slice(0, 55)}`);
        if (d.silenceDurationMs !== null) lines.push(`Silence: ${d.silenceDurationMs}ms`);
        if (d.instruction) lines.push(`Instr: ${d.instruction.slice(0, 65)}...`);
        setTooltip({ x: mx, y: my, lines });
      })
      .on('mouseout', function (_event: MouseEvent, d) {
        d3.select(this).attr('stroke', 'none');
        d3.select(this).attr(
          'opacity',
          selectedIteration !== null && d.iteration !== selectedIteration
            ? 0.4
            : 1,
        );
        setTooltip(null);
      });

    // Iteration labels
    g.selectAll<SVGTextElement, IterationDetailEntry>('text.label')
      .data(entries)
      .join('text')
      .attr('class', 'label')
      .attr('x', -5)
      .attr('y', (d) => (yScale(d.iteration) ?? 0) + yScale.bandwidth() / 2)
      .attr('text-anchor', 'end')
      .attr('dominant-baseline', 'middle')
      .attr('fill', '#9ca3af')
      .attr('font-size', '11px')
      .attr('font-family', 'ui-monospace, monospace')
      .text((d) => `#${d.iteration}`);

    // Stall indicators
    entries.forEach((e, i) => {
      const cx = xScale((timestamps[i] ?? 0) - startTime);
      const cy = (yScale(e.iteration) ?? 0) + yScale.bandwidth() / 2;
      if (e.blockExecuting) {
        g.append('circle')
          .attr('cx', cx - 8)
          .attr('cy', cy)
          .attr('r', 3.5)
          .attr('fill', '#FF5050');
      }
      if (e.silenceDurationMs !== null && e.silenceDurationMs < 2000) {
        g.append('circle')
          .attr('cx', cx - (e.blockExecuting ? 16 : 8))
          .attr('cy', cy)
          .attr('r', 3.5)
          .attr('fill', '#DCA03C');
      }
    });
  }, [entries, selectedIteration, onSelectIteration]);

  if (entries.length === 0) return null;

  return (
    <div ref={containerRef} className="relative">
      <svg ref={svgRef} />
      {tooltip && (
        <div
          className="absolute z-10 bg-gray-800 border border-gray-700 text-xs text-gray-200 px-3 py-2 rounded shadow-lg pointer-events-none max-w-sm"
          style={{ left: tooltip.x + 14, top: tooltip.y - 10 }}
        >
          {tooltip.lines.map((line, i) => (
            <div key={i} className={i === 0 ? 'font-mono font-semibold mb-0.5' : 'text-gray-400'}>
              {line}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

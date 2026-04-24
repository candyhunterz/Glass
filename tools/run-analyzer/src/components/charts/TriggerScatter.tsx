import { useRef, useEffect, useState } from 'react';
import * as d3 from 'd3';
import type { IterationDetailEntry } from '../../types';
import { getTriggerColor, TRIGGER_COLORS } from '../../utils/colors';

interface Props {
  entries: IterationDetailEntry[];
  silenceTimeoutMs?: number;
}

interface TooltipState {
  x: number;
  y: number;
  lines: string[];
}

export function TriggerScatter({ entries, silenceTimeoutMs = 10000 }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  const withTrigger = entries.filter(
    (e) => e.triggerSource && e.silenceDurationMs !== null,
  );

  useEffect(() => {
    if (!svgRef.current || withTrigger.length === 0) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const margin = { top: 20, right: 20, bottom: 45, left: 70 };
    const width = 700 - margin.left - margin.right;
    const height = 370 - margin.top - margin.bottom;

    svg
      .attr('width', width + margin.left + margin.right)
      .attr('height', height + margin.top + margin.bottom);

    const g = svg
      .append('g')
      .attr('transform', `translate(${margin.left},${margin.top})`);

    const maxIter = d3.max(withTrigger, (d) => d.iteration) ?? 1;
    const maxSilence = d3.max(withTrigger, (d) => d.silenceDurationMs ?? 0) ?? 1;
    const yMax = Math.max(maxSilence * 1.1, silenceTimeoutMs * 1.3);

    const xScale = d3.scaleLinear().domain([0, maxIter]).range([0, width]).nice();
    const yScale = d3.scaleLinear().domain([0, yMax]).range([height, 0]).nice();

    // Grid lines
    g.append('g')
      .attr('class', 'grid')
      .selectAll('line')
      .data(yScale.ticks(6))
      .join('line')
      .attr('x1', 0)
      .attr('x2', width)
      .attr('y1', (d) => yScale(d))
      .attr('y2', (d) => yScale(d))
      .attr('stroke', '#1f2937')
      .attr('stroke-dasharray', '2,3');

    // Axes
    const xAxisG = g
      .append('g')
      .attr('transform', `translate(0,${height})`)
      .call(d3.axisBottom(xScale).ticks(10));
    xAxisG.selectAll('text').attr('fill', '#9ca3af');
    xAxisG.selectAll('.domain, .tick line').attr('stroke', '#374151');

    const yAxisG = g.append('g').call(d3.axisLeft(yScale).ticks(8));
    yAxisG.selectAll('text').attr('fill', '#9ca3af');
    yAxisG.selectAll('.domain, .tick line').attr('stroke', '#374151');

    // Axis labels
    g.append('text')
      .attr('x', width / 2)
      .attr('y', height + 38)
      .attr('text-anchor', 'middle')
      .attr('fill', '#6b7280')
      .attr('font-size', '11px')
      .text('Iteration');

    g.append('text')
      .attr('transform', 'rotate(-90)')
      .attr('x', -height / 2)
      .attr('y', -55)
      .attr('text-anchor', 'middle')
      .attr('fill', '#6b7280')
      .attr('font-size', '11px')
      .text('Silence (ms)');

    // Reference line at silence timeout
    const refY = yScale(silenceTimeoutMs);
    if (refY >= 0 && refY <= height) {
      g.append('line')
        .attr('x1', 0)
        .attr('x2', width)
        .attr('y1', refY)
        .attr('y2', refY)
        .attr('stroke', '#FF5050')
        .attr('stroke-width', 1)
        .attr('stroke-dasharray', '6,4')
        .attr('opacity', 0.6);
      g.append('text')
        .attr('x', width - 2)
        .attr('y', refY - 5)
        .attr('text-anchor', 'end')
        .attr('fill', '#FF5050')
        .attr('font-size', '9px')
        .attr('opacity', 0.7)
        .text(`timeout ${silenceTimeoutMs}ms`);
    }

    const container = containerRef.current;

    // Data points — circles for normal, triangles for block executing
    for (const d of withTrigger) {
      if (d.silenceDurationMs === null) continue;
      const cx = xScale(d.iteration);
      const cy = yScale(d.silenceDurationMs);
      const color = getTriggerColor(d.triggerSource ?? '');

      const showTip = (event: MouseEvent) => {
        if (!container) return;
        const [mx, my] = d3.pointer(event, container);
        const lines = [
          `#${d.iteration}`,
          `Trigger: ${d.triggerSource?.split(',')[0]?.trim() ?? 'unknown'}`,
          `Silence: ${d.silenceDurationMs}ms`,
        ];
        if (d.blockExecuting) lines.push('BLOCK STILL EXECUTING');
        setTooltip({ x: mx, y: my, lines });
      };

      if (d.blockExecuting) {
        g.append('path')
          .attr('d', `M${cx},${cy - 6} L${cx + 6},${cy + 5} L${cx - 6},${cy + 5}Z`)
          .attr('fill', color)
          .attr('opacity', 0.85)
          .attr('cursor', 'pointer')
          .on('mouseover', function (this: SVGPathElement, event: MouseEvent) {
            d3.select(this).attr('opacity', 1).attr('stroke', '#fff').attr('stroke-width', 1);
            showTip(event);
          })
          .on('mouseout', function (this: SVGPathElement) {
            d3.select(this).attr('opacity', 0.85).attr('stroke', 'none');
            setTooltip(null);
          });
      } else {
        g.append('circle')
          .attr('cx', cx)
          .attr('cy', cy)
          .attr('r', 4.5)
          .attr('fill', color)
          .attr('opacity', 0.75)
          .attr('cursor', 'pointer')
          .on('mouseover', function (this: SVGCircleElement, event: MouseEvent) {
            d3.select(this).attr('opacity', 1).attr('stroke', '#fff').attr('stroke-width', 1);
            showTip(event);
          })
          .on('mouseout', function (this: SVGCircleElement) {
            d3.select(this).attr('opacity', 0.75).attr('stroke', 'none');
            setTooltip(null);
          });
      }
    }

    // Legend
    const legendY = -10;
    const sources = Object.entries(TRIGGER_COLORS);
    sources.forEach(([name, color], i) => {
      const lx = width - sources.length * 90 + i * 90;
      g.append('circle').attr('cx', lx).attr('cy', legendY).attr('r', 4).attr('fill', color);
      g.append('text')
        .attr('x', lx + 8)
        .attr('y', legendY)
        .attr('dominant-baseline', 'middle')
        .attr('fill', '#9ca3af')
        .attr('font-size', '10px')
        .text(name);
    });
  }, [withTrigger, silenceTimeoutMs]);

  if (withTrigger.length === 0) return null;

  return (
    <div ref={containerRef} className="relative">
      <svg ref={svgRef} />
      {tooltip && (
        <div
          className="absolute z-10 bg-gray-800 border border-gray-700 text-xs text-gray-200 px-3 py-2 rounded shadow-lg pointer-events-none"
          style={{ left: tooltip.x + 14, top: tooltip.y - 10 }}
        >
          {tooltip.lines.map((line, i) => (
            <div
              key={i}
              className={
                i === 0
                  ? 'font-mono font-semibold'
                  : line === 'BLOCK STILL EXECUTING'
                    ? 'text-red-400 font-medium'
                    : 'text-gray-400'
              }
            >
              {line}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

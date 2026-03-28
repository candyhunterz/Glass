import { useRef, useEffect, useState } from 'react';
import * as d3 from 'd3';
import type { RunMetricsEntry } from '../../types';

interface Props {
  runs: RunMetricsEntry[];
}

interface TooltipState {
  x: number;
  y: number;
  lines: string[];
}

const RATE_LINES = [
  { key: 'waste_rate', color: '#DCA03C', label: 'Waste' },
  { key: 'stuck_rate', color: '#FF5050', label: 'Stuck' },
  { key: 'revert_rate', color: '#6478FF', label: 'Revert' },
  { key: 'checkpoint_rate', color: '#B48CFF', label: 'Checkpoint' },
] as const;

function getRateValue(run: RunMetricsEntry, key: string): number {
  switch (key) {
    case 'waste_rate':
      return run.waste_rate;
    case 'stuck_rate':
      return run.stuck_rate;
    case 'revert_rate':
      return run.revert_rate;
    case 'checkpoint_rate':
      return run.checkpoint_rate;
    default:
      return 0;
  }
}

export function RatesTrend({ runs }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  useEffect(() => {
    if (!svgRef.current || runs.length === 0) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const margin = { top: 15, right: 110, bottom: 35, left: 50 };
    const width = 650 - margin.left - margin.right;
    const height = 280 - margin.top - margin.bottom;

    svg
      .attr('width', width + margin.left + margin.right)
      .attr('height', height + margin.top + margin.bottom);

    const g = svg
      .append('g')
      .attr('transform', `translate(${margin.left},${margin.top})`);

    const xScale = d3
      .scaleLinear()
      .domain([0, Math.max(runs.length - 1, 1)])
      .range([0, width]);
    const yScale = d3.scaleLinear().domain([0, 1]).range([height, 0]);

    // Grid
    g.append('g')
      .selectAll('line')
      .data(yScale.ticks(5))
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
      .call(
        d3
          .axisBottom(xScale)
          .ticks(runs.length)
          .tickFormat((_d, i) => `#${i + 1}`),
      );
    xAxisG.selectAll('text').attr('fill', '#9ca3af').attr('font-size', '10px');
    xAxisG.selectAll('.domain, .tick line').attr('stroke', '#374151');

    const yAxisG = g
      .append('g')
      .call(d3.axisLeft(yScale).ticks(5).tickFormat(d3.format('.0%')));
    yAxisG.selectAll('text').attr('fill', '#9ca3af').attr('font-size', '10px');
    yAxisG.selectAll('.domain, .tick line').attr('stroke', '#374151');

    const container = containerRef.current;

    // Lines + dots for each rate
    for (const rate of RATE_LINES) {
      const lineGen = d3
        .line<RunMetricsEntry>()
        .x((_d, i) => xScale(i))
        .y((d) => yScale(getRateValue(d, rate.key)));

      g.append('path')
        .datum(runs)
        .attr('fill', 'none')
        .attr('stroke', rate.color)
        .attr('stroke-width', 2)
        .attr('d', lineGen);

      // Interactive dots
      runs.forEach((run, i) => {
        const cx = xScale(i);
        const cy = yScale(getRateValue(run, rate.key));
        g.append('circle')
          .attr('cx', cx)
          .attr('cy', cy)
          .attr('r', 4)
          .attr('fill', rate.color)
          .attr('cursor', 'pointer')
          .on('mouseover', function (event: MouseEvent) {
            d3.select(this).attr('r', 6).attr('stroke', '#fff').attr('stroke-width', 1.5);
            if (!container) return;
            const [mx, my] = d3.pointer(event, container);
            setTooltip({
              x: mx,
              y: my,
              lines: [
                `Run #${i + 1}: ${run.run_id}`,
                ...RATE_LINES.map(
                  (r) =>
                    `${r.label}: ${(getRateValue(run, r.key) * 100).toFixed(1)}%`,
                ),
              ],
            });
          })
          .on('mouseout', function () {
            d3.select(this).attr('r', 4).attr('stroke', 'none');
            setTooltip(null);
          });
      });
    }

    // Legend
    RATE_LINES.forEach((rate, i) => {
      const ly = 10 + i * 20;
      g.append('line')
        .attr('x1', width + 15)
        .attr('x2', width + 30)
        .attr('y1', ly)
        .attr('y2', ly)
        .attr('stroke', rate.color)
        .attr('stroke-width', 2);
      g.append('text')
        .attr('x', width + 35)
        .attr('y', ly)
        .attr('dominant-baseline', 'middle')
        .attr('fill', '#9ca3af')
        .attr('font-size', '10px')
        .text(rate.label);
    });
  }, [runs]);

  if (runs.length === 0) return null;

  return (
    <div ref={containerRef} className="relative">
      <svg ref={svgRef} />
      {tooltip && (
        <div
          className="absolute z-10 bg-gray-800 border border-gray-700 text-xs text-gray-200 px-3 py-2 rounded shadow-lg pointer-events-none"
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

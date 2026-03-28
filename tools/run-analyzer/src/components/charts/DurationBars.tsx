import { useRef, useEffect, useState } from 'react';
import * as d3 from 'd3';
import type { RunMetricsEntry } from '../../types';
import { formatDuration } from '../../utils/format';

interface Props {
  runs: RunMetricsEntry[];
}

interface TooltipState {
  x: number;
  y: number;
  lines: string[];
}

export function DurationBars({ runs }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  useEffect(() => {
    if (!svgRef.current || runs.length === 0) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const margin = { top: 15, right: 55, bottom: 35, left: 55 };
    const width = 650 - margin.left - margin.right;
    const height = 260 - margin.top - margin.bottom;

    svg
      .attr('width', width + margin.left + margin.right)
      .attr('height', height + margin.top + margin.bottom);

    const g = svg
      .append('g')
      .attr('transform', `translate(${margin.left},${margin.top})`);

    // Scales
    const xScale = d3
      .scaleBand<number>()
      .domain(runs.map((_r, i) => i))
      .range([0, width])
      .padding(0.3);

    const maxDur = d3.max(runs, (r) => r.duration_secs) ?? 1;
    const yScaleDur = d3
      .scaleLinear()
      .domain([0, maxDur])
      .range([height, 0])
      .nice();

    const maxIter = d3.max(runs, (r) => r.iterations) ?? 1;
    const yScaleIter = d3
      .scaleLinear()
      .domain([0, maxIter * 1.2])
      .range([height, 0])
      .nice();

    // X axis
    const xAxisG = g
      .append('g')
      .attr('transform', `translate(0,${height})`)
      .call(
        d3.axisBottom(xScale).tickFormat((_d, i) => `#${i + 1}`),
      );
    xAxisG.selectAll('text').attr('fill', '#9ca3af').attr('font-size', '10px');
    xAxisG.selectAll('.domain, .tick line').attr('stroke', '#374151');

    // Left Y axis — duration
    const yAxisL = g.append('g').call(d3.axisLeft(yScaleDur).ticks(5));
    yAxisL.selectAll('text').attr('fill', '#9ca3af').attr('font-size', '10px');
    yAxisL.selectAll('.domain, .tick line').attr('stroke', '#374151');
    g.append('text')
      .attr('transform', 'rotate(-90)')
      .attr('x', -height / 2)
      .attr('y', -42)
      .attr('text-anchor', 'middle')
      .attr('fill', '#6478FF')
      .attr('font-size', '10px')
      .text('Duration (s)');

    // Right Y axis — iterations
    const yAxisR = g
      .append('g')
      .attr('transform', `translate(${width},0)`)
      .call(d3.axisRight(yScaleIter).ticks(5));
    yAxisR.selectAll('text').attr('fill', '#9ca3af').attr('font-size', '10px');
    yAxisR.selectAll('.domain, .tick line').attr('stroke', '#374151');
    g.append('text')
      .attr('transform', 'rotate(90)')
      .attr('x', height / 2)
      .attr('y', -(width + 42))
      .attr('text-anchor', 'middle')
      .attr('fill', '#B48CFF')
      .attr('font-size', '10px')
      .text('Iterations');

    const container = containerRef.current;

    // Duration bars
    runs.forEach((run, i) => {
      g.append('rect')
        .attr('x', xScale(i) ?? 0)
        .attr('y', yScaleDur(run.duration_secs))
        .attr('width', xScale.bandwidth())
        .attr('height', height - yScaleDur(run.duration_secs))
        .attr('fill', '#6478FF')
        .attr('rx', 2)
        .attr('opacity', 0.75)
        .attr('cursor', 'pointer')
        .on('mouseover', function (event: MouseEvent) {
          d3.select(this).attr('opacity', 1).attr('stroke', '#fff').attr('stroke-width', 1);
          if (!container) return;
          const [mx, my] = d3.pointer(event, container);
          setTooltip({
            x: mx,
            y: my,
            lines: [
              `Run #${i + 1}: ${run.run_id}`,
              `Duration: ${formatDuration(run.duration_secs)}`,
              `Iterations: ${run.iterations}`,
            ],
          });
        })
        .on('mouseout', function () {
          d3.select(this).attr('opacity', 0.75).attr('stroke', 'none');
          setTooltip(null);
        });
    });

    // Iteration count overlay line
    const lineGen = d3
      .line<RunMetricsEntry>()
      .x((_d, i) => (xScale(i) ?? 0) + xScale.bandwidth() / 2)
      .y((d) => yScaleIter(d.iterations));

    g.append('path')
      .datum(runs)
      .attr('fill', 'none')
      .attr('stroke', '#B48CFF')
      .attr('stroke-width', 2)
      .attr('d', lineGen);

    // Iteration count dots
    runs.forEach((run, i) => {
      g.append('circle')
        .attr('cx', (xScale(i) ?? 0) + xScale.bandwidth() / 2)
        .attr('cy', yScaleIter(run.iterations))
        .attr('r', 3.5)
        .attr('fill', '#B48CFF');
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

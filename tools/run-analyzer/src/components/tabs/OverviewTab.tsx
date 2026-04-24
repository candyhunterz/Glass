import { useRef, useEffect } from 'react';
import type { ReactNode } from 'react';
import * as d3 from 'd3';
import { useDataStore } from '../../stores/dataStore';
import type { RunReport } from '../../types';

function Card({
  label,
  value,
  children,
}: {
  label: string;
  value: string | number;
  children?: ReactNode;
}) {
  return (
    <div className="bg-gray-900 rounded-lg p-4 border border-gray-800">
      <p className="text-xs font-medium text-gray-500 uppercase tracking-wide mb-1">
        {label}
      </p>
      {value !== '' && (
        <p className="text-2xl font-mono font-bold text-gray-100">{value}</p>
      )}
      {children}
    </div>
  );
}

interface TriggerDatum {
  label: string;
  value: number;
  color: string;
}

function TriggerDonut({ report }: { report: RunReport }) {
  const svgRef = useRef<SVGSVGElement>(null);

  const data: TriggerDatum[] = [
    { label: 'Prompt', value: report.triggerPrompt, color: '#509678' },
    { label: 'ShellPrompt', value: report.triggerShellPrompt, color: '#6478FF' },
    { label: 'Fast', value: report.triggerFast, color: '#DCA03C' },
    { label: 'Slow', value: report.triggerSlow, color: '#64646E' },
  ].filter((d) => d.value > 0);

  useEffect(() => {
    if (!svgRef.current || data.length === 0) return;

    const size = 180;
    const radius = size / 2;
    const svg = d3.select(svgRef.current).attr('width', size).attr('height', size);
    svg.selectAll('*').remove();

    const g = svg.append('g').attr('transform', `translate(${radius},${radius})`);

    const pieGen = d3
      .pie<TriggerDatum>()
      .value((d) => d.value)
      .sort(null);

    const arcGen = d3
      .arc<d3.PieArcDatum<TriggerDatum>>()
      .innerRadius(radius * 0.55)
      .outerRadius(radius * 0.9);

    g.selectAll('path')
      .data(pieGen(data))
      .join('path')
      .attr('d', (d) => arcGen(d) ?? '')
      .attr('fill', (d) => d.data.color);

    g.append('text')
      .attr('text-anchor', 'middle')
      .attr('dy', '0.35em')
      .attr('fill', '#e5e7eb')
      .attr('font-size', '1.4rem')
      .attr('font-family', 'monospace')
      .text(String(report.triggerTotal));
  }, [data, report.triggerTotal]);

  if (data.length === 0) {
    return <p className="text-gray-500 text-sm">No trigger data.</p>;
  }

  return (
    <div className="flex items-center gap-6">
      <svg ref={svgRef} />
      <div className="space-y-2 text-sm">
        {data.map((d) => (
          <div key={d.label} className="flex items-center gap-2">
            <span
              className="w-3 h-3 rounded-sm inline-block shrink-0"
              style={{ backgroundColor: d.color }}
            />
            <span className="text-gray-400">{d.label}</span>
            <span className="text-gray-200 font-mono">{d.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function OverviewTab() {
  const report = useDataStore((s) => {
    const file = s.selectedRunFile;
    return file ? (s.runReports[file] ?? null) : null;
  });
  const tsvEntries = useDataStore((s) => s.tsvEntries);

  if (!report) {
    return <p className="text-gray-500">No run report loaded.</p>;
  }

  const breakdown = {
    instruction: tsvEntries.filter((e) => e.status === 'instruction').length,
    wait: tsvEntries.filter((e) => e.status === 'wait').length,
    stuck: tsvEntries.filter((e) => e.status === 'stuck').length,
    checkpoint: tsvEntries.filter((e) => e.status === 'checkpoint').length,
  };

  const denom = report.iterations + report.stuckEvents + report.reverts;
  const efficiency = denom > 0 ? (report.commits * 10) / denom : 0;
  const effColor =
    efficiency > 5 ? '#509678' : efficiency >= 2 ? '#DCA03C' : '#FF5050';
  const stuckPct =
    report.iterations > 0
      ? ((report.stuckEvents / report.iterations) * 100).toFixed(1)
      : '0.0';

  return (
    <div className="space-y-8">
      <div className="grid grid-cols-3 gap-4">
        <Card label="Iterations" value={report.iterations}>
          <span className="text-xs text-gray-500">
            {breakdown.instruction} instr / {breakdown.wait} wait /{' '}
            {breakdown.stuck} stuck / {breakdown.checkpoint} ckpt
          </span>
        </Card>
        <Card label="Duration" value={report.duration} />
        <Card label="Commits" value={report.commits}>
          <span className="text-xs text-gray-500">
            {report.iterationsPerCommit} iter/commit
          </span>
        </Card>
        <Card
          label="Metric Guard"
          value={`${report.baselinesEstablished}B / ${report.keeps}K / ${report.reverts}R`}
        >
          <span className="text-xs text-gray-500">
            {report.testsPassed} passed, {report.testsFailed} failed
          </span>
        </Card>
        <Card label="Stuck Events" value={report.stuckEvents}>
          <span className="text-xs text-gray-500">{stuckPct}%</span>
        </Card>
        <Card label="Completion" value="">
          <span className="text-sm text-gray-300 line-clamp-3">
            {report.completion || 'Unknown'}
          </span>
        </Card>
      </div>

      <div className="grid grid-cols-2 gap-6">
        <div className="bg-gray-900 rounded-lg p-5 border border-gray-800">
          <h3 className="text-sm font-medium text-gray-400 mb-4">
            Trigger Sources
          </h3>
          <TriggerDonut report={report} />
        </div>
        <div className="bg-gray-900 rounded-lg p-5 border border-gray-800">
          <h3 className="text-sm font-medium text-gray-400 mb-4">
            Efficiency Score
          </h3>
          <div className="flex items-center justify-center h-36">
            <div className="text-center">
              <span
                className="text-5xl font-mono font-bold"
                style={{ color: effColor }}
              >
                {efficiency.toFixed(1)}
              </span>
              <p className="text-xs text-gray-500 mt-2">
                (commits x 10) / (iterations + stuck + reverts)
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

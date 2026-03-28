import { useDataStore } from '../../stores/dataStore';
import { formatDuration, formatPercent } from '../../utils/format';
import { RatesTrend } from '../charts/RatesTrend';
import { DurationBars } from '../charts/DurationBars';

export function CrossRunTab() {
  const runMetrics = useDataStore((s) => s.runMetrics);
  const runs = runMetrics?.runs ?? [];

  if (runs.length === 0) {
    return <p className="text-gray-500">No run metrics loaded.</p>;
  }

  return (
    <div className="space-y-8">
      <div className="grid grid-cols-2 gap-6">
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-4">
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Rates Trend
          </h3>
          <div className="overflow-x-auto">
            <RatesTrend runs={runs} />
          </div>
        </div>
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-4">
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Duration Trend
          </h3>
          <div className="overflow-x-auto">
            <DurationBars runs={runs} />
          </div>
        </div>
      </div>

      <div>
        <h3 className="text-sm font-medium text-gray-400 mb-3">
          Summary Table
        </h3>
        <div className="bg-gray-900 border border-gray-800 rounded-lg overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-gray-500 text-left border-b border-gray-800">
                <th className="px-4 py-2">Run ID</th>
                <th className="px-4 py-2 text-right">Iterations</th>
                <th className="px-4 py-2 text-right">Duration</th>
                <th className="px-4 py-2 text-right">Waste %</th>
                <th className="px-4 py-2 text-right">Stuck %</th>
                <th className="px-4 py-2">Completion</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <tr key={run.run_id} className="border-t border-gray-800/50">
                  <td className="px-4 py-2 font-mono text-xs text-gray-300">
                    {run.run_id}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-gray-300">
                    {run.iterations}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-gray-300">
                    {formatDuration(run.duration_secs)}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-gray-300">
                    {formatPercent(run.waste_rate)}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-gray-300">
                    {formatPercent(run.stuck_rate)}
                  </td>
                  <td className="px-4 py-2 text-gray-400 text-xs truncate max-w-xs">
                    {run.completion.split(':').slice(0, 2).join(':')}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

import { useDataStore } from '../../stores/dataStore';
import { getTriggerColor } from '../../utils/colors';
import { TriggerScatter } from '../charts/TriggerScatter';

export function TriggersTab() {
  const entries = useDataStore((s) => s.iterationDetails);

  const withTrigger = entries.filter((e) => e.triggerSource);
  const stallEntries = entries.filter((e) => e.blockExecuting);
  const stallPct =
    entries.length > 0
      ? ((stallEntries.length / entries.length) * 100).toFixed(1)
      : '0.0';

  // Trigger source distribution
  const distribution: Record<string, number> = {};
  for (const e of withTrigger) {
    const type = e.triggerSource?.split(',')[0]?.trim() ?? 'Unknown';
    distribution[type] = (distribution[type] ?? 0) + 1;
  }

  if (entries.length === 0) {
    return <p className="text-gray-500">No iteration details loaded.</p>;
  }

  return (
    <div className="space-y-8">
      <div>
        <h3 className="text-sm font-medium text-gray-400 mb-3">
          Trigger Scatter Plot
        </h3>
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-4 overflow-x-auto">
          <div className="flex gap-4 mb-3 text-xs text-gray-500">
            <span className="flex items-center gap-1">
              <span className="w-2 h-2 rounded-full inline-block bg-gray-400" />{' '}
              Circle = normal
            </span>
            <span className="flex items-center gap-1">
              <span className="w-0 h-0 inline-block border-l-[4px] border-r-[4px] border-b-[7px] border-transparent border-b-gray-400" />{' '}
              Triangle = block executing
            </span>
          </div>
          <TriggerScatter entries={entries} />
        </div>
      </div>

      <div className="grid grid-cols-2 gap-6">
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-5">
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Trigger Distribution
          </h3>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-gray-500 text-left">
                <th className="pb-2">Source</th>
                <th className="pb-2 text-right">Count</th>
              </tr>
            </thead>
            <tbody>
              {Object.entries(distribution)
                .sort(([, a], [, b]) => b - a)
                .map(([source, count]) => (
                  <tr key={source} className="border-t border-gray-800">
                    <td className="py-1.5">
                      <span className="flex items-center gap-2">
                        <span
                          className="w-2.5 h-2.5 rounded-sm inline-block"
                          style={{ backgroundColor: getTriggerColor(source) }}
                        />
                        {source}
                      </span>
                    </td>
                    <td className="py-1.5 text-right font-mono text-gray-300">
                      {count}
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>

        <div className="bg-gray-900 border border-gray-800 rounded-lg p-5">
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Stall Report
          </h3>
          <p className="text-2xl font-mono font-bold text-gray-100 mb-1">
            {stallEntries.length}{' '}
            <span className="text-sm font-normal text-gray-500">
              stalls ({stallPct}%)
            </span>
          </p>
          {parseFloat(stallPct) > 20 && (
            <p className="text-yellow-400 text-xs mb-3">
              High stall rate — investigate rendering pipeline
            </p>
          )}
          {stallEntries.length > 0 && (
            <div className="max-h-48 overflow-y-auto mt-3">
              <table className="w-full text-xs">
                <thead>
                  <tr className="text-gray-500 text-left">
                    <th className="pb-1">#</th>
                    <th className="pb-1">Trigger</th>
                    <th className="pb-1 text-right">Silence</th>
                  </tr>
                </thead>
                <tbody>
                  {stallEntries.map((e) => (
                    <tr key={e.iteration} className="border-t border-gray-800">
                      <td className="py-1 font-mono">{e.iteration}</td>
                      <td className="py-1 text-gray-400 truncate max-w-[150px]">
                        {e.triggerSource?.split(',')[0]?.trim() ?? '-'}
                      </td>
                      <td className="py-1 text-right font-mono text-gray-300">
                        {e.silenceDurationMs ?? '-'}ms
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

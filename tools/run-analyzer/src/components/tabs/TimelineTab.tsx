import { useDataStore } from '../../stores/dataStore';
import { useUiStore } from '../../stores/uiStore';
import { getStatusColor } from '../../utils/colors';
import { IterationTimeline } from '../charts/IterationTimeline';

export function TimelineTab() {
  const entries = useDataStore((s) => s.iterationDetails);
  const selectedIteration = useUiStore((s) => s.selectedIteration);
  const setSelectedIteration = useUiStore((s) => s.setSelectedIteration);

  const selectedEntry = entries.find((e) => e.iteration === selectedIteration);

  if (entries.length === 0) {
    return <p className="text-gray-500">No iteration details loaded.</p>;
  }

  return (
    <div className="flex gap-6">
      <div className="flex-1 overflow-x-auto">
        <div className="flex items-center gap-4 mb-4 text-xs text-gray-500">
          <span className="flex items-center gap-1">
            <span className="w-2.5 h-2.5 rounded-full bg-red-500 inline-block" />{' '}
            Block executing
          </span>
          <span className="flex items-center gap-1">
            <span
              className="w-2.5 h-2.5 rounded-full inline-block"
              style={{ backgroundColor: '#DCA03C' }}
            />{' '}
            Fast trigger (&lt;2s)
          </span>
        </div>
        <IterationTimeline
          entries={entries}
          selectedIteration={selectedIteration}
          onSelectIteration={setSelectedIteration}
        />
      </div>

      {selectedEntry && (
        <div className="w-80 shrink-0 bg-gray-900 border border-gray-800 rounded-lg p-4 text-sm space-y-3 self-start">
          <div className="flex justify-between items-center">
            <h3 className="font-mono font-semibold text-gray-200">
              Iteration #{selectedEntry.iteration}
            </h3>
            <button
              onClick={() => setSelectedIteration(null)}
              className="text-gray-500 hover:text-gray-300"
            >
              x
            </button>
          </div>
          <p className="text-gray-400">
            <span className="text-gray-500">Time:</span>{' '}
            {selectedEntry.timestamp}
          </p>
          {selectedEntry.action && (
            <p>
              <span className="text-gray-500">Action:</span>{' '}
              <span
                className="px-1.5 py-0.5 rounded text-xs font-mono"
                style={{
                  backgroundColor: getStatusColor(selectedEntry.action) + '22',
                  color: getStatusColor(selectedEntry.action),
                }}
              >
                {selectedEntry.action}
              </span>
            </p>
          )}
          {selectedEntry.triggerSource && (
            <p className="text-gray-400">
              <span className="text-gray-500">Trigger:</span>{' '}
              {selectedEntry.triggerSource}
            </p>
          )}
          {selectedEntry.silenceDurationMs !== null && (
            <p className="text-gray-400">
              <span className="text-gray-500">Silence:</span>{' '}
              {selectedEntry.silenceDurationMs}ms
            </p>
          )}
          {selectedEntry.instruction && (
            <div>
              <p className="text-gray-500 mb-1">Instruction:</p>
              <p className="text-gray-300 text-xs leading-relaxed line-clamp-6">
                {selectedEntry.instruction}
              </p>
            </div>
          )}
          {selectedEntry.filesChanged.length > 0 && (
            <div>
              <p className="text-gray-500 mb-1">Files changed:</p>
              <ul className="text-gray-400 text-xs space-y-0.5">
                {selectedEntry.filesChanged.map((f) => (
                  <li key={f} className="font-mono">
                    {f}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {selectedEntry.verifyResult && (
            <p className="text-gray-400">
              <span className="text-gray-500">Verify:</span>{' '}
              {selectedEntry.verifyResult}
            </p>
          )}
          {selectedEntry.blockExecuting && (
            <p className="text-red-400 text-xs font-medium">
              BLOCK STILL EXECUTING
            </p>
          )}
          {selectedEntry.note && (
            <p className="text-gray-500 text-xs">{selectedEntry.note}</p>
          )}
        </div>
      )}
    </div>
  );
}

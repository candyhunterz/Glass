import { useDataStore } from '../stores/dataStore';

export function RunSelector() {
  const runReports = useDataStore((s) => s.runReports);
  const selectedRunFile = useDataStore((s) => s.selectedRunFile);
  const selectRun = useDataStore((s) => s.selectRun);

  const files = Object.keys(runReports).sort();
  if (files.length <= 1) return null;

  return (
    <select
      value={selectedRunFile ?? ''}
      onChange={(e) => selectRun(e.target.value)}
      className="bg-gray-800 text-gray-200 border border-gray-700 rounded px-3 py-1.5 text-sm font-mono"
    >
      {files.map((f) => (
        <option key={f} value={f}>
          {f.replace(/\.md$/, '')}
        </option>
      ))}
    </select>
  );
}

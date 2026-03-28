import { useDataStore } from '../../stores/dataStore';
import { formatPercent } from '../../utils/format';
import { RuleSankey } from '../charts/RuleSankey';

function StatusBadge({ status }: { status: string }) {
  const colors: Record<string, string> = {
    provisional: 'bg-yellow-900/50 text-yellow-400 border-yellow-700',
    confirmed: 'bg-green-900/50 text-green-400 border-green-700',
    rejected: 'bg-red-900/50 text-red-400 border-red-700',
  };
  return (
    <span
      className={`px-1.5 py-0.5 rounded text-xs font-mono border ${colors[status] ?? 'bg-gray-800 text-gray-400 border-gray-700'}`}
    >
      {status}
    </span>
  );
}

function SeverityBadge({ severity }: { severity: string }) {
  const colors: Record<string, string> = {
    high: 'text-red-400',
    medium: 'text-yellow-400',
    low: 'text-gray-400',
  };
  return (
    <span className={`text-xs font-mono ${colors[severity] ?? 'text-gray-500'}`}>
      {severity}
    </span>
  );
}

function TierCard({
  tier,
  label,
  active,
  detail,
}: {
  tier: number;
  label: string;
  active: boolean;
  detail: string;
}) {
  return (
    <div
      className={`rounded-lg p-4 border ${
        active
          ? 'bg-green-950/30 border-green-800'
          : 'bg-gray-900 border-gray-800'
      }`}
    >
      <p className="text-xs text-gray-500 mb-1">Tier {tier}</p>
      <p
        className={`text-sm font-medium ${active ? 'text-green-400' : 'text-gray-500'}`}
      >
        {label}
      </p>
      <p className="text-xs text-gray-500 mt-1">{detail}</p>
    </div>
  );
}

export function FeedbackTab() {
  const rules = useDataStore((s) => s.rules);
  const archivedRules = useDataStore((s) => s.archivedRules);
  const tuningHistory = useDataStore((s) => s.tuningHistory);
  const runMetrics = useDataStore((s) => s.runMetrics);
  const report = useDataStore((s) => {
    const file = s.selectedRunFile;
    return file ? (s.runReports[file] ?? null) : null;
  });

  const allRules = [...rules, ...archivedRules];

  // Get rule firings for the latest run from metrics
  const latestRun = runMetrics?.runs[runMetrics.runs.length - 1];
  const firings = latestRun?.rule_firings ?? [];
  const activeFirings = firings.filter((f) => f.firing_count > 0);

  // Determine tier activity from the report's feedback markdown
  const fb = report?.feedbackMarkdown ?? '';
  const tier1Active =
    fb.includes('Tier 1') && !fb.includes('No config changes');
  const tier2Active =
    fb.includes('new finding') || activeFirings.length > 0;
  const tier3Active =
    fb.includes('Tier 3') && !fb.includes('No prompt hints');
  const tier4Active =
    fb.includes('Tier 4') && !fb.includes('Not triggered');

  return (
    <div className="space-y-8">
      {/* Tier Activity */}
      <div>
        <h3 className="text-sm font-medium text-gray-400 mb-3">
          Tier Activity
        </h3>
        <div className="grid grid-cols-4 gap-4">
          <TierCard
            tier={1}
            label="Config Tuning"
            active={tier1Active}
            detail={tier1Active ? 'Changes applied' : 'No changes'}
          />
          <TierCard
            tier={2}
            label="Behavioral Rules"
            active={tier2Active}
            detail={
              tier2Active
                ? `${activeFirings.length} rule(s) fired`
                : 'No activity'
            }
          />
          <TierCard
            tier={3}
            label="Prompt Hints"
            active={tier3Active}
            detail={tier3Active ? 'Active' : 'Inactive'}
          />
          <TierCard
            tier={4}
            label="Script Generation"
            active={tier4Active}
            detail={tier4Active ? 'Triggered' : 'Not triggered'}
          />
        </div>
      </div>

      {/* Rule Lifecycle Flow */}
      {allRules.length > 0 && (
        <div>
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Rule Lifecycle Flow
          </h3>
          <div className="bg-gray-900 border border-gray-800 rounded-lg p-4 overflow-x-auto">
            <RuleSankey rules={allRules} />
          </div>
        </div>
      )}

      {/* Config Tuning History */}
      {tuningHistory && (
        <div>
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Config Tuning History
          </h3>
          <div className="bg-gray-900 border border-gray-800 rounded-lg overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-gray-500 text-left border-b border-gray-800">
                  <th className="px-4 py-2">Run</th>
                  <th className="px-4 py-2">Config Values</th>
                </tr>
              </thead>
              <tbody>
                {tuningHistory.snapshots.map((snap) => (
                  <tr
                    key={snap.run_id}
                    className="border-t border-gray-800/50"
                  >
                    <td className="px-4 py-2 font-mono text-xs text-gray-400">
                      {snap.run_id}
                    </td>
                    <td className="px-4 py-2 text-xs text-gray-300">
                      {Object.entries(snap.config_values)
                        .map(([k, v]) => `${k}=${v}`)
                        .join(', ')}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {tuningHistory.pending && (
              <div className="px-4 py-3 bg-yellow-950/20 border-t border-yellow-800/50">
                <p className="text-xs text-yellow-400">
                  Pending: {tuningHistory.pending.field}{' '}
                  {tuningHistory.pending.old_value} →{' '}
                  {tuningHistory.pending.new_value}
                  <span className="text-yellow-600 ml-2">
                    (finding: {tuningHistory.pending.finding_id})
                  </span>
                </p>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Rule Lifecycle Table */}
      <div>
        <h3 className="text-sm font-medium text-gray-400 mb-3">
          Rule Lifecycle
          <span className="text-gray-600 ml-2 text-xs">
            ({allRules.length} rules)
          </span>
        </h3>
        {allRules.length === 0 ? (
          <p className="text-gray-500 text-sm">No rules defined.</p>
        ) : (
          <div className="bg-gray-900 border border-gray-800 rounded-lg overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-gray-500 text-left border-b border-gray-800">
                  <th className="px-4 py-2">ID</th>
                  <th className="px-4 py-2">Status</th>
                  <th className="px-4 py-2">Action</th>
                  <th className="px-4 py-2">Severity</th>
                  <th className="px-4 py-2">Added Run</th>
                  <th className="px-4 py-2 text-right">Firings</th>
                </tr>
              </thead>
              <tbody>
                {allRules.map((rule) => (
                  <tr key={rule.id} className="border-t border-gray-800/50">
                    <td className="px-4 py-2 font-mono text-xs text-gray-300">
                      {rule.id}
                    </td>
                    <td className="px-4 py-2">
                      <StatusBadge status={rule.status} />
                    </td>
                    <td className="px-4 py-2 text-gray-400 text-xs">
                      {rule.action}
                    </td>
                    <td className="px-4 py-2">
                      <SeverityBadge severity={rule.severity} />
                    </td>
                    <td className="px-4 py-2 font-mono text-xs text-gray-500">
                      {rule.added_run}
                    </td>
                    <td className="px-4 py-2 text-right font-mono text-gray-300">
                      {rule.trigger_count}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Rule Firings for selected run */}
      {firings.length > 0 && (
        <div>
          <h3 className="text-sm font-medium text-gray-400 mb-3">
            Rule Firings (Latest Run)
          </h3>
          <div className="bg-gray-900 border border-gray-800 rounded-lg p-4">
            <div className="space-y-2">
              {firings.map((f) => {
                const maxCount = Math.max(
                  ...firings.map((r) => r.firing_count),
                  1,
                );
                const barWidth = (f.firing_count / maxCount) * 100;
                return (
                  <div key={f.rule_id} className="flex items-center gap-3">
                    <span className="w-36 text-xs font-mono text-gray-400 shrink-0">
                      {f.rule_id}
                    </span>
                    <div className="flex-1 bg-gray-800 rounded-full h-4 overflow-hidden">
                      <div
                        className="h-full rounded-full bg-blue-500/70"
                        style={{ width: `${barWidth}%` }}
                      />
                    </div>
                    <span className="text-xs font-mono text-gray-300 w-12 text-right">
                      {f.firing_count}
                    </span>
                  </div>
                );
              })}
            </div>
            <p className="text-xs text-gray-500 mt-3">
              Total firings:{' '}
              {firings.reduce((sum, f) => sum + f.firing_count, 0)} | Active:{' '}
              {activeFirings.length} | Waste rate:{' '}
              {latestRun ? formatPercent(latestRun.waste_rate) : 'N/A'}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

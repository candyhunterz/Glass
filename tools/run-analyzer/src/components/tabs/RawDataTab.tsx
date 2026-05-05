import { useState } from 'react';
import { marked } from 'marked';
import { useDataStore } from '../../stores/dataStore';

function renderMarkdown(src: string): string {
  return marked.parse(src) as string;
}

function RenderedMarkdown({ source }: { source: string }) {
  return (
    <div
      className="markdown-content"
      dangerouslySetInnerHTML={{ __html: renderMarkdown(source) }}
    />
  );
}

function CollapsibleIteration({
  title,
  content,
}: {
  title: string;
  content: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border-b border-gray-800">
      <button
        onClick={() => setOpen(!open)}
        className="w-full text-left px-4 py-2 text-sm font-mono text-gray-300 hover:bg-gray-800/50 flex items-center gap-2"
      >
        <span className="text-gray-500">{open ? '\u25BC' : '\u25B6'}</span>
        {title}
      </button>
      {open && (
        <div className="px-4 py-3 bg-gray-900/50 text-sm">
          <RenderedMarkdown source={content} />
        </div>
      )}
    </div>
  );
}

export function RawDataTab() {
  const rawFiles = useDataStore((s) => s.rawFiles);
  const selectedRunFile = useDataStore((s) => s.selectedRunFile);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<'report' | 'details' | 'raw'>(
    'report',
  );

  const fileNames = Object.keys(rawFiles).sort();
  const runReportContent = selectedRunFile ? (rawFiles[selectedRunFile] ?? '') : '';
  const iterationDetailsContent = rawFiles['iteration-details.md'] ?? '';

  // Split iteration details into sections
  const iterationSections: { title: string; content: string }[] = [];
  if (iterationDetailsContent) {
    const parts = iterationDetailsContent.split(/(?=^## Iteration \d+)/m);
    for (const part of parts) {
      const headerMatch = part.match(/^## (Iteration \d+ \[\d{2}:\d{2}:\d{2}\])/);
      if (headerMatch) {
        iterationSections.push({
          title: headerMatch[1]!,
          content: part,
        });
      }
    }
  }

  return (
    <div className="space-y-4">
      <div className="flex gap-2">
        <button
          onClick={() => setViewMode('report')}
          className={`px-3 py-1.5 rounded text-sm ${
            viewMode === 'report'
              ? 'bg-gray-700 text-white'
              : 'bg-gray-800 text-gray-400 hover:text-gray-200'
          }`}
        >
          Run Report
        </button>
        <button
          onClick={() => setViewMode('details')}
          className={`px-3 py-1.5 rounded text-sm ${
            viewMode === 'details'
              ? 'bg-gray-700 text-white'
              : 'bg-gray-800 text-gray-400 hover:text-gray-200'
          }`}
        >
          Iteration Details
        </button>
        <button
          onClick={() => setViewMode('raw')}
          className={`px-3 py-1.5 rounded text-sm ${
            viewMode === 'raw'
              ? 'bg-gray-700 text-white'
              : 'bg-gray-800 text-gray-400 hover:text-gray-200'
          }`}
        >
          Raw Files
        </button>
      </div>

      {viewMode === 'report' && (
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-6">
          {runReportContent ? (
            <RenderedMarkdown source={runReportContent} />
          ) : (
            <p className="text-gray-500">No run report loaded.</p>
          )}
        </div>
      )}

      {viewMode === 'details' && (
        <div className="bg-gray-900 border border-gray-800 rounded-lg overflow-hidden">
          {iterationSections.length > 0 ? (
            iterationSections.map((sec) => (
              <CollapsibleIteration
                key={sec.title}
                title={sec.title}
                content={sec.content}
              />
            ))
          ) : (
            <p className="text-gray-500 p-4">
              No iteration details loaded.
            </p>
          )}
        </div>
      )}

      {viewMode === 'raw' && (
        <div>
          <select
            value={selectedFile ?? ''}
            onChange={(e) => setSelectedFile(e.target.value || null)}
            className="bg-gray-800 text-gray-200 border border-gray-700 rounded px-3 py-1.5 text-sm font-mono mb-4"
          >
            <option value="">Select a file...</option>
            {fileNames.map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
          {selectedFile && rawFiles[selectedFile] && (
            <pre className="bg-gray-900 border border-gray-800 rounded-lg p-4 overflow-x-auto text-xs font-mono text-gray-300 whitespace-pre-wrap max-h-[70vh]">
              {rawFiles[selectedFile]}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

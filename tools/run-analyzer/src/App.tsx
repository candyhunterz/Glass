import { useRef, useCallback, useEffect, useState } from 'react';
import { useDataStore } from './stores/dataStore';
import { useUiStore } from './stores/uiStore';
import { detectServerSource } from './dataSources';
import { FolderLoader } from './components/FolderLoader';
import { RunSelector } from './components/RunSelector';
import { OverviewTab } from './components/tabs/OverviewTab';
import { TimelineTab } from './components/tabs/TimelineTab';
import { TriggersTab } from './components/tabs/TriggersTab';
import { FeedbackTab } from './components/tabs/FeedbackTab';
import { CrossRunTab } from './components/tabs/CrossRunTab';
import { RawDataTab } from './components/tabs/RawDataTab';

const TABS = [
  { id: 'overview', label: 'Overview' },
  { id: 'timeline', label: 'Timeline' },
  { id: 'triggers', label: 'Triggers' },
  { id: 'feedback', label: 'Feedback' },
  { id: 'crossrun', label: 'Cross-Run' },
  { id: 'raw', label: 'Raw Data' },
] as const;

function OpenFolderButton() {
  const loadFiles = useDataStore((s) => s.loadFiles);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.setAttribute('webkitdirectory', '');
  }, []);

  const handleOpen = useCallback(async () => {
    if (window.showDirectoryPicker) {
      try {
        const handle = await window.showDirectoryPicker();
        const files: Record<string, string> = {};
        for await (const entry of handle.values()) {
          if (entry.kind === 'file') {
            const file = await (entry as FileSystemFileHandle).getFile();
            files[entry.name] = await file.text();
          }
        }
        loadFiles(files);
      } catch {
        // User cancelled
      }
    } else {
      inputRef.current?.click();
    }
  }, [loadFiles]);

  return (
    <>
      <button
        onClick={handleOpen}
        className="px-3 py-1.5 bg-gray-800 text-gray-300 border border-gray-700 rounded text-sm hover:bg-gray-700 transition-colors"
      >
        Open Folder
      </button>
      <input
        ref={inputRef}
        type="file"
        multiple
        className="hidden"
        onChange={async (e) => {
          if (!e.target.files) return;
          const files: Record<string, string> = {};
          for (const f of e.target.files) {
            files[f.name] = await f.text();
          }
          loadFiles(files);
        }}
      />
    </>
  );
}

function TabContent({ tab }: { tab: string }) {
  switch (tab) {
    case 'overview':
      return <OverviewTab />;
    case 'timeline':
      return <TimelineTab />;
    case 'triggers':
      return <TriggersTab />;
    case 'feedback':
      return <FeedbackTab />;
    case 'crossrun':
      return <CrossRunTab />;
    case 'raw':
      return <RawDataTab />;
    default:
      return null;
  }
}

function App() {
  const isLoaded = useDataStore((s) => s.isLoaded);
  const dataSourceLabel = useDataStore((s) => s.dataSourceLabel);
  const loadFromSource = useDataStore((s) => s.loadFromSource);
  const activeTab = useUiStore((s) => s.activeTab);
  const setActiveTab = useUiStore((s) => s.setActiveTab);
  const [detecting, setDetecting] = useState(true);

  useEffect(() => {
    let cancelled = false;
    detectServerSource().then((source) => {
      if (cancelled) return;
      if (source) {
        loadFromSource(source);
      }
      setDetecting(false);
    });
    return () => { cancelled = true; };
  }, [loadFromSource]);

  if (detecting && !isLoaded) {
    return (
      <div className="min-h-screen bg-gray-950 text-gray-100 flex items-center justify-center">
        <p className="text-gray-400">Loading...</p>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-950 text-gray-100 flex flex-col">
      <header className="flex items-center justify-between border-b border-gray-800 px-6 py-3 shrink-0">
        <div className="flex items-center gap-3">
          <h1 className="text-lg font-mono font-semibold tracking-tight">
            Glass Run Analyzer
          </h1>
          {dataSourceLabel && (
            <span className="text-xs text-gray-500 font-mono">
              Loaded from: {dataSourceLabel}
            </span>
          )}
        </div>
        {isLoaded && (
          <div className="flex items-center gap-3">
            <RunSelector />
            <OpenFolderButton />
          </div>
        )}
      </header>

      {!isLoaded ? (
        <FolderLoader />
      ) : (
        <>
          <nav className="flex border-b border-gray-800 px-6 shrink-0">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`px-4 py-2.5 text-sm font-medium transition-colors ${
                  activeTab === tab.id
                    ? 'text-white border-b-2 border-blue-500'
                    : 'text-gray-400 hover:text-gray-200'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </nav>
          <main className="flex-1 overflow-auto p-6">
            <TabContent tab={activeTab} />
          </main>
        </>
      )}
    </div>
  );
}

export default App;

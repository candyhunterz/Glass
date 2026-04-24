import { useRef, useState, useCallback, useEffect } from 'react';
import { useDataStore } from '../stores/dataStore';

async function readDirectoryHandle(
  handle: FileSystemDirectoryHandle,
): Promise<Record<string, string>> {
  const files: Record<string, string> = {};
  for await (const entry of handle.values()) {
    if (entry.kind === 'file') {
      const file = await (entry as FileSystemFileHandle).getFile();
      files[entry.name] = await file.text();
    }
  }
  return files;
}

export function FolderLoader() {
  const loadFiles = useDataStore((s) => s.loadFiles);
  const isLoading = useDataStore((s) => s.isLoading);
  const error = useDataStore((s) => s.error);
  const [isDragOver, setIsDragOver] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.setAttribute('webkitdirectory', '');
  }, []);

  const handleOpen = useCallback(async () => {
    if (window.showDirectoryPicker) {
      try {
        const handle = await window.showDirectoryPicker();
        const files = await readDirectoryHandle(handle);
        loadFiles(files);
      } catch {
        // User cancelled picker
      }
    } else {
      inputRef.current?.click();
    }
  }, [loadFiles]);

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragOver(false);

      const item = e.dataTransfer.items[0];
      if (item && 'getAsFileSystemHandle' in item) {
        try {
          const handle = await (
            item as unknown as {
              getAsFileSystemHandle(): Promise<FileSystemHandle>;
            }
          ).getAsFileSystemHandle();
          if (handle.kind === 'directory') {
            const files = await readDirectoryHandle(
              handle as FileSystemDirectoryHandle,
            );
            loadFiles(files);
            return;
          }
        } catch {
          // Fall through to file-based reading
        }
      }

      const files: Record<string, string> = {};
      for (const file of e.dataTransfer.files) {
        files[file.name] = await file.text();
      }
      if (Object.keys(files).length > 0) {
        loadFiles(files);
      }
    },
    [loadFiles],
  );

  const handleInputChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      if (!e.target.files) return;
      const files: Record<string, string> = {};
      for (const f of e.target.files) {
        files[f.name] = await f.text();
      }
      loadFiles(files);
    },
    [loadFiles],
  );

  return (
    <div className="flex-1 flex items-center justify-center p-8">
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setIsDragOver(true);
        }}
        onDragLeave={() => setIsDragOver(false)}
        onDrop={handleDrop}
        className={`w-full max-w-lg border-2 border-dashed rounded-xl p-12 text-center transition-colors ${
          isDragOver
            ? 'border-blue-400 bg-blue-400/10'
            : 'border-gray-700 bg-gray-900/50'
        }`}
      >
        <svg
          className="w-12 h-12 mx-auto mb-4 text-gray-600"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={1.5}
            d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
          />
        </svg>
        <p className="text-gray-400 mb-2">
          Drop a{' '}
          <code className="px-1.5 py-0.5 bg-gray-800 rounded text-gray-300 text-sm font-mono">
            .glass/
          </code>{' '}
          folder here
        </p>
        <p className="text-gray-500 text-sm mb-6">or</p>
        <button
          onClick={handleOpen}
          disabled={isLoading}
          className="px-5 py-2.5 bg-blue-600 text-white rounded-lg font-medium hover:bg-blue-500 disabled:opacity-50 transition-colors"
        >
          {isLoading ? 'Loading...' : 'Open Folder'}
        </button>
        <input
          ref={inputRef}
          type="file"
          multiple
          className="hidden"
          onChange={handleInputChange}
        />
        {error && <p className="mt-4 text-red-400 text-sm">{error}</p>}
      </div>
    </div>
  );
}

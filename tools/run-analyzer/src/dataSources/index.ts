export type { DataSource } from './types';
export { ServerDataSource } from './serverSource';
export { FilesystemDataSource } from './filesystemSource';

import { ServerDataSource } from './serverSource';

/**
 * Try to detect a server data source at /api/files.
 * Returns a ServerDataSource if the endpoint responds 200, null otherwise.
 */
export async function detectServerSource(): Promise<ServerDataSource | null> {
  try {
    const res = await fetch('/api/files');
    if (res.ok) {
      const source = new ServerDataSource();
      await source.init();
      return source;
    }
  } catch {
    // Not served by glass analyze — fall back to folder picker
  }
  return null;
}

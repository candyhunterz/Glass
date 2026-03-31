import type { DataSource } from './types';

export class ServerDataSource implements DataSource {
  private dirLabel = '';

  async init(): Promise<void> {
    try {
      const res = await fetch('/api/dir');
      if (res.ok) {
        const data = (await res.json()) as { path: string };
        this.dirLabel = data.path;
      }
    } catch {
      // Label stays empty if /api/dir unavailable
    }
  }

  async listFiles(): Promise<string[]> {
    const res = await fetch('/api/files');
    if (!res.ok) throw new Error(`Failed to list files: ${res.status}`);
    return (await res.json()) as string[];
  }

  async readFile(name: string): Promise<string> {
    const res = await fetch(`/api/files/${encodeURIComponent(name)}`);
    if (!res.ok) throw new Error(`Failed to read ${name}: ${res.status}`);
    return res.text();
  }

  label(): string {
    return this.dirLabel || 'Server';
  }
}

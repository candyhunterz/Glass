import type { DataSource } from './types';

export class FilesystemDataSource implements DataSource {
  private files: Record<string, string>;

  constructor(files: Record<string, string>) {
    this.files = files;
  }

  async listFiles(): Promise<string[]> {
    return Object.keys(this.files);
  }

  async readFile(name: string): Promise<string> {
    const content = this.files[name];
    if (content === undefined) throw new Error(`File not found: ${name}`);
    return content;
  }

  label(): string {
    return 'Local folder';
  }
}

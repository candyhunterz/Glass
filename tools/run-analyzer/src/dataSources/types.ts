export interface DataSource {
  listFiles(): Promise<string[]>;
  readFile(name: string): Promise<string>;
  label(): string;
}

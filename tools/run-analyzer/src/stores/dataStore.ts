import { create } from 'zustand';
import type {
  RunReport,
  IterationDetailEntry,
  TsvEntry,
  RunMetrics,
  Rule,
  TuningHistory,
} from '../types';
import type { DataSource } from '../dataSources/types';
import { parseRunReport } from '../parsers/runReport';
import { parseIterationDetails } from '../parsers/iterationDetails';
import { parseIterationsTsv } from '../parsers/iterationsTsv';
import { parseRunMetrics } from '../parsers/runMetrics';
import { parseRules, parseArchivedRules } from '../parsers/rules';
import { parseTuningHistory } from '../parsers/tuningHistory';

interface DataState {
  runReports: Record<string, RunReport>;
  iterationDetails: IterationDetailEntry[];
  tsvEntries: TsvEntry[];
  runMetrics: RunMetrics | null;
  rules: Rule[];
  archivedRules: Rule[];
  tuningHistory: TuningHistory | null;
  rawFiles: Record<string, string>;
  selectedRunFile: string | null;
  isLoading: boolean;
  isLoaded: boolean;
  error: string | null;
  dataSourceLabel: string | null;

  loadFiles: (files: Record<string, string>) => void;
  loadFromSource: (source: DataSource) => Promise<void>;
  selectRun: (filename: string) => void;
  reset: () => void;
}

const INITIAL_STATE = {
  runReports: {} as Record<string, RunReport>,
  iterationDetails: [] as IterationDetailEntry[],
  tsvEntries: [] as TsvEntry[],
  runMetrics: null as RunMetrics | null,
  rules: [] as Rule[],
  archivedRules: [] as Rule[],
  tuningHistory: null as TuningHistory | null,
  rawFiles: {} as Record<string, string>,
  selectedRunFile: null as string | null,
  isLoading: false,
  isLoaded: false,
  error: null as string | null,
  dataSourceLabel: null as string | null,
};

export const useDataStore = create<DataState>()((set) => ({
  ...INITIAL_STATE,

  loadFiles: (files) => {
    set({ isLoading: true, error: null });
    try {
      const runReports: Record<string, RunReport> = {};
      let iterationDetails: IterationDetailEntry[] = [];
      let tsvEntries: TsvEntry[] = [];
      let runMetrics: RunMetrics | null = null;
      let rules: Rule[] = [];
      let archivedRules: Rule[] = [];
      let tuningHistory: TuningHistory | null = null;

      for (const [name, content] of Object.entries(files)) {
        if (/^run-report-.*\.md$/.test(name)) {
          runReports[name] = parseRunReport(content);
        } else if (name === 'iteration-details.md') {
          iterationDetails = parseIterationDetails(content);
        } else if (name === 'iterations.tsv') {
          tsvEntries = parseIterationsTsv(content);
        } else if (name === 'run-metrics.toml') {
          runMetrics = parseRunMetrics(content);
        } else if (name === 'rules.toml') {
          rules = parseRules(content);
        } else if (name === 'archived-rules.toml') {
          archivedRules = parseArchivedRules(content);
        } else if (name === 'tuning-history.toml') {
          tuningHistory = parseTuningHistory(content);
        }
      }

      // Auto-select the most recent run report (last alphabetically)
      const reportFiles = Object.keys(runReports).sort();
      const selectedRunFile = reportFiles[reportFiles.length - 1] ?? null;

      set({
        runReports,
        iterationDetails,
        tsvEntries,
        runMetrics,
        rules,
        archivedRules,
        tuningHistory,
        rawFiles: files,
        selectedRunFile,
        isLoading: false,
        isLoaded: true,
      });
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  loadFromSource: async (source) => {
    set({ isLoading: true, error: null });
    try {
      const names = await source.listFiles();
      const files: Record<string, string> = {};
      for (const name of names) {
        files[name] = await source.readFile(name);
      }

      const runReports: Record<string, RunReport> = {};
      let iterationDetails: IterationDetailEntry[] = [];
      let tsvEntries: TsvEntry[] = [];
      let runMetrics: RunMetrics | null = null;
      let rules: Rule[] = [];
      let archivedRules: Rule[] = [];
      let tuningHistory: TuningHistory | null = null;

      for (const [name, content] of Object.entries(files)) {
        if (/^run-report-.*\.md$/.test(name)) {
          runReports[name] = parseRunReport(content);
        } else if (name === 'iteration-details.md') {
          iterationDetails = parseIterationDetails(content);
        } else if (name === 'iterations.tsv') {
          tsvEntries = parseIterationsTsv(content);
        } else if (name === 'run-metrics.toml') {
          runMetrics = parseRunMetrics(content);
        } else if (name === 'rules.toml') {
          rules = parseRules(content);
        } else if (name === 'archived-rules.toml') {
          archivedRules = parseArchivedRules(content);
        } else if (name === 'tuning-history.toml') {
          tuningHistory = parseTuningHistory(content);
        }
      }

      const reportFiles = Object.keys(runReports).sort();
      const selectedRunFile = reportFiles[reportFiles.length - 1] ?? null;

      set({
        runReports,
        iterationDetails,
        tsvEntries,
        runMetrics,
        rules,
        archivedRules,
        tuningHistory,
        rawFiles: files,
        selectedRunFile,
        isLoading: false,
        isLoaded: true,
        dataSourceLabel: source.label(),
      });
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  selectRun: (filename) => set({ selectedRunFile: filename }),

  reset: () => set({ ...INITIAL_STATE }),
}));

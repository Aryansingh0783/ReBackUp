import { create } from 'zustand';
import { api, events } from '@/lib/api';
import type {
  FileFilter, FileRow, QueryResult, ScanOptions, ScanProgress, ScanSummary, SortKey, TreeNode,
} from '@/types';

const EMPTY_FILTER: FileFilter = { extensions: [], includeDirs: false };

interface ScanState {
  scanId: string | null;
  running: boolean;
  paused: boolean;
  progress: ScanProgress | null;
  summary: ScanSummary | null;
  error: string | null;

  tree: TreeNode | null;
  /** Drill-down breadcrumb: [root, ...ancestors, current] */
  crumbs: TreeNode[];

  filter: FileFilter;
  sort: SortKey;
  desc: boolean;
  rows: FileRow[];
  totalMatches: number;
  totalBytes: number;
  selected: Set<string>;

  start: (options: ScanOptions) => Promise<void>;
  togglePause: () => Promise<void>;
  cancel: () => Promise<void>;
  drill: (node: number) => Promise<void>;
  crumbTo: (index: number) => Promise<void>;
  setFilter: (patch: Partial<FileFilter>) => Promise<void>;
  setSort: (sort: SortKey, desc: boolean) => Promise<void>;
  refreshRows: () => Promise<void>;
  toggleSelect: (path: string) => void;
  selectAllVisible: () => void;
  clearSelection: () => void;
}

let unlisteners: Array<() => void> = [];

export const useScan = create<ScanState>((set, get) => ({
  scanId: null,
  running: false,
  paused: false,
  progress: null,
  summary: null,
  error: null,
  tree: null,
  crumbs: [],
  filter: EMPTY_FILTER,
  sort: 'size',
  desc: true,
  rows: [],
  totalMatches: 0,
  totalBytes: 0,
  selected: new Set<string>(),

  start: async (options) => {
    unlisteners.forEach((u) => u());
    unlisteners = [];
    set({
      running: true, paused: false, error: null, progress: null,
      summary: null, tree: null, crumbs: [], rows: [],
    });

    try {
      const scanId = await api.startScan(options);
      set({ scanId });

      unlisteners.push(
        await events.scanProgress((p) => {
          if (p.scanId === get().scanId) set({ progress: p });
        }),
      );
      unlisteners.push(
        await events.scanDone(async (s) => {
          if (s.scanId !== get().scanId) return;
          set({ summary: s, running: false });
          if (s.files > 0 || s.dirs > 0) {
            const tree = await api.scanTree(s.scanId, s.root, 2, 24);
            set({ tree, crumbs: [tree] });
            await get().refreshRows();
          }
        }),
      );
    } catch (e) {
      set({ running: false, error: (e as Error).message });
    }
  },

  togglePause: async () => {
    const { scanId, paused } = get();
    if (!scanId) return;
    await api.setScanPaused(scanId, !paused);
    set({ paused: !paused });
  },

  cancel: async () => {
    const { scanId } = get();
    if (!scanId) return;
    await api.cancelScan(scanId);
    set({ running: false, paused: false });
  },

  drill: async (node) => {
    const { scanId, crumbs } = get();
    if (!scanId || node === 0xffffffff) return;
    const tree = await api.scanTree(scanId, node, 2, 24);
    set({ tree, crumbs: [...crumbs, tree] });
    await get().setFilter({ under: node });
  },

  crumbTo: async (index) => {
    const { scanId, crumbs } = get();
    if (!scanId || index < 0 || index >= crumbs.length) return;
    const target = crumbs[index];
    const tree = await api.scanTree(scanId, target.id, 2, 24);
    set({ tree, crumbs: crumbs.slice(0, index + 1) });
    await get().setFilter({ under: index === 0 ? null : target.id });
  },

  setFilter: async (patch) => {
    set({ filter: { ...get().filter, ...patch } });
    await get().refreshRows();
  },

  setSort: async (sort, desc) => {
    set({ sort, desc });
    await get().refreshRows();
  },

  refreshRows: async () => {
    const { scanId, filter, sort, desc } = get();
    if (!scanId) return;
    try {
      const r: QueryResult = await api.queryFiles(scanId, filter, sort, desc, 0, 1000);
      set({ rows: r.rows, totalMatches: r.totalMatches, totalBytes: r.totalBytes });
    } catch (e) {
      set({ error: (e as Error).message });
    }
  },

  toggleSelect: (path) => {
    const next = new Set(get().selected);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    set({ selected: next });
  },
  selectAllVisible: () => {
    const next = new Set(get().selected);
    get().rows.forEach((r) => next.add(r.path));
    set({ selected: next });
  },
  clearSelection: () => set({ selected: new Set<string>() }),
}));

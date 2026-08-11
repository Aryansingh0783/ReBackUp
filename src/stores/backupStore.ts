import { create } from 'zustand';
import { api, events } from '@/lib/api';
import type {
  ArchiveMode, BackupPlan, BackupProgress, BackupResult, BackupSelection,
  Detection, LogLine, Profile,
} from '@/types';

interface BackupState {
  profiles: Profile[];
  detections: Detection[];
  enabled: Set<string>;
  customIncludes: string[];
  excludedPaths: string[];
  archive: ArchiveMode;
  outputDir: string | null;
  runGitStatus: boolean;

  plan: BackupPlan | null;
  planning: boolean;
  running: boolean;
  progress: BackupProgress | null;
  log: LogLine[];
  result: BackupResult | null;
  error: string | null;

  loadProfiles: () => Promise<void>;
  toggleProfile: (id: string) => void;
  setArchive: (m: ArchiveMode) => void;
  setOutputDir: (d: string | null) => void;
  setRunGitStatus: (v: boolean) => void;
  addCustomInclude: (pattern: string) => void;
  removeCustomInclude: (pattern: string) => void;
  setExcluded: (paths: string[]) => void;
  selection: (extraPaths: string[]) => BackupSelection;
  buildPlan: (extraPaths: string[]) => Promise<void>;
  run: (extraPaths: string[], passphrase: string) => Promise<void>;
  cancel: () => Promise<void>;
  reset: () => void;
}

let unlisteners: Array<() => void> = [];

export const useBackup = create<BackupState>((set, get) => ({
  profiles: [],
  detections: [],
  enabled: new Set<string>(),
  customIncludes: [],
  excludedPaths: [],
  archive: 'sevenZip',
  outputDir: null,
  runGitStatus: true,

  plan: null,
  planning: false,
  running: false,
  progress: null,
  log: [],
  result: null,
  error: null,

  loadProfiles: async () => {
    try {
      const [profiles, detections] = await Promise.all([
        api.listProfiles(),
        api.detectTargets(),
      ]);
      // Default-on, but only for things that actually exist on this machine.
      const found = new Set(detections.filter((d) => d.found).map((d) => d.id));
      const enabled = new Set(
        profiles
          .filter((p) => p.enabledByDefault && (found.has(p.id) || p.include.length === 0))
          .map((p) => p.id),
      );
      set({ profiles, detections, enabled });
    } catch (e) {
      set({ error: (e as Error).message });
    }
  },

  toggleProfile: (id) => {
    const next = new Set(get().enabled);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    set({ enabled: next, plan: null });
  },

  setArchive: (archive) => set({ archive }),
  setOutputDir: (outputDir) => set({ outputDir, plan: null }),
  setRunGitStatus: (runGitStatus) => set({ runGitStatus }),

  addCustomInclude: (pattern) => {
    const p = pattern.trim();
    if (!p || get().customIncludes.includes(p)) return;
    const enabled = new Set(get().enabled);
    enabled.add('custom');
    set({ customIncludes: [...get().customIncludes, p], enabled, plan: null });
  },
  removeCustomInclude: (pattern) =>
    set({ customIncludes: get().customIncludes.filter((p) => p !== pattern), plan: null }),
  setExcluded: (excludedPaths) => set({ excludedPaths, plan: null }),

  selection: (extraPaths) => ({
    profileIds: [...get().enabled],
    extraPaths,
    excludedPaths: get().excludedPaths,
    customIncludes: get().customIncludes,
    archive: get().archive,
    outputDir: get().outputDir,
    runGitStatus: get().runGitStatus,
  }),

  buildPlan: async (extraPaths) => {
    set({ planning: true, error: null });
    try {
      const plan = await api.planBackup(get().selection(extraPaths));
      set({ plan, planning: false });
    } catch (e) {
      set({ error: (e as Error).message, planning: false });
    }
  },

  run: async (extraPaths, passphrase) => {
    const plan = get().plan;
    if (!plan) return;
    unlisteners.forEach((u) => u());
    unlisteners = [];
    set({ running: true, log: [], progress: null, result: null, error: null });

    unlisteners.push(await events.backupProgress((p) => set({ progress: p })));
    unlisteners.push(
      await events.backupLog((l) => set({ log: [...get().log, l].slice(-500) })),
    );

    try {
      // The passphrase lives only in this call's arguments — never in the store.
      const result = await api.runBackup(plan.id, get().selection(extraPaths), passphrase);
      set({ result, running: false });
    } catch (e) {
      set({ error: (e as Error).message, running: false });
    }
  },

  cancel: async () => {
    await api.cancelBackup();
    set({ running: false });
  },

  reset: () => set({ plan: null, result: null, progress: null, log: [], error: null }),
}));

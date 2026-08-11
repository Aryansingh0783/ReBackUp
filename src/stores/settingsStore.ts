import { create } from 'zustand';
import { api } from '@/lib/api';
import type { Environment, DriveInfo } from '@/types';

interface SettingsState {
  env: Environment | null;
  drives: DriveInfo[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useSettings = create<SettingsState>((set) => ({
  env: null,
  drives: [],
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const [env, drives] = await Promise.all([api.environment(), api.listDrives()]);
      set({ env, drives, loading: false });
    } catch (e) {
      set({ error: (e as Error).message, loading: false });
    }
  },
}));

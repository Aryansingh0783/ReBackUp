/**
 * Typed wrappers around the Tauri command surface.
 *
 * Two rules enforced here:
 *  - Rust returns `AppError` as `{ kind, message }`; `call` normalises anything
 *    else so the UI only ever handles one error shape.
 *  - Passphrases are passed straight through and never stored in a zustand
 *    store, logged, or put in component state that survives the modal.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type * as T from '@/types';

export class ApiError extends Error {
  kind: T.AppError['kind'];
  constructor(e: T.AppError) {
    super(e.message);
    this.name = 'ApiError';
    this.kind = e.kind;
  }
}

async function call<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  try {
    return await invoke<R>(cmd, args);
  } catch (e) {
    if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
      throw new ApiError(e as T.AppError);
    }
    throw new ApiError({ kind: 'other', message: String(e) });
  }
}

export const api = {
  environment: () => call<T.Environment>('environment'),
  listDrives: () => call<T.DriveInfo[]>('list_drives'),

  startScan: (options: T.ScanOptions) => call<string>('start_scan', { options }),
  setScanPaused: (scanId: string, paused: boolean) =>
    call<void>('set_scan_paused', { scanId, paused }),
  cancelScan: (scanId: string) => call<void>('cancel_scan', { scanId }),
  scanSummary: (scanId: string) => call<T.ScanSummary>('scan_summary', { scanId }),
  scanTree: (scanId: string, node?: number, depth = 2, fanout = 24) =>
    call<T.TreeNode>('scan_tree', { scanId, node, depth, fanout }),
  queryFiles: (
    scanId: string,
    filter: T.FileFilter,
    sort: T.SortKey = 'size',
    desc = true,
    offset = 0,
    limit = 500,
  ) => call<T.QueryResult>('query_files', { scanId, filter, sort, desc, offset, limit }),
  gitReposFromScan: (scanId: string, runStatus: boolean, limit = 2000) =>
    call<T.GitReport>('git_repos_from_scan', { scanId, runStatus, limit }),

  listProfiles: () => call<T.Profile[]>('list_profiles'),
  saveProfiles: (profiles: T.Profile[]) => call<void>('save_profiles', { profiles }),
  detectTargets: () => call<T.Detection[]>('detect_targets'),
  detectBrowsers: () => call<T.BrowserProfile[]>('detect_browsers'),
  detectSteam: () => call<T.SteamReport>('detect_steam'),
  discoverGit: (roots: string[], runStatus: boolean) =>
    call<T.GitReport>('discover_git', { roots, runStatus }),
  credentialManagerInfo: () => call<T.VaultInfo>('credential_manager_info'),
  openCredentialWizard: () => call<void>('open_credential_wizard'),
  passwordManagerUrl: (browser: string) => call<string>('password_manager_url', { browser }),

  checkPassphrase: (passphrase: string) => call<void>('check_passphrase', { passphrase }),
  planBackup: (selection: T.BackupSelection) => call<T.BackupPlan>('plan_backup', { selection }),
  runBackup: (planId: string, selection: T.BackupSelection, passphrase: string) =>
    call<T.BackupResult>('run_backup', { planId, selection, passphrase }),
  cancelBackup: () => call<void>('cancel_backup'),
  verifyBackup: (manifestPath: string) => call<T.VerifyResult>('verify_backup', { manifestPath }),
  sealExportedCsv: (
    staging: string,
    csvPath: string,
    label: string,
    passphrase: string,
    shredSource: boolean,
  ) => call<T.SealedArtifact>('seal_exported_csv', { staging, csvPath, label, passphrase, shredSource }),
  unsealTo: (sealedPath: string, outPath: string, passphrase: string) =>
    call<number>('unseal_to', { sealedPath, outPath, passphrase }),
  shredFile: (path: string) => call<void>('shred_file', { path }),
};

export const events = {
  scanProgress: (cb: (p: T.ScanProgress) => void): Promise<UnlistenFn> =>
    listen<T.ScanProgress>('scan://progress', (e) => cb(e.payload)),
  scanDone: (cb: (s: T.ScanSummary) => void): Promise<UnlistenFn> =>
    listen<T.ScanSummary>('scan://done', (e) => cb(e.payload)),
  backupProgress: (cb: (p: T.BackupProgress) => void): Promise<UnlistenFn> =>
    listen<T.BackupProgress>('backup://progress', (e) => cb(e.payload)),
  backupLog: (cb: (l: T.LogLine) => void): Promise<UnlistenFn> =>
    listen<T.LogLine>('backup://log', (e) => cb(e.payload)),
  backupDone: (cb: (r: T.BackupResult) => void): Promise<UnlistenFn> =>
    listen<T.BackupResult>('backup://done', (e) => cb(e.payload)),
};

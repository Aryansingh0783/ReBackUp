/** Mirrors the `serde(rename_all = "camelCase")` shapes emitted by src-tauri. */

export interface AppError {
  kind:
    | 'io' | 'serde' | 'access_denied' | 'windows_only' | 'not_ntfs'
    | 'parse' | 'unknown_scan' | 'crypto' | 'integrity' | 'other';
  message: string;
}

export interface DriveInfo {
  name: string;
  mount: string;
  fileSystem: string;
  totalBytes: number;
  freeBytes: number;
  removable: boolean;
  ntfs: boolean;
}

export interface Environment {
  elevated: boolean;
  windows: boolean;
  mftAvailable: boolean;
  tempDir: string;
  homeDir: string;
  user: string;
  version: string;
}

export type ScanBackend = 'mft' | 'walk' | 'auto';

export interface ScanOptions {
  target: string;
  backend?: ScanBackend;
  threads?: number;
}

export interface ScanProgress {
  scanId: string;
  done: number;
  total: number;
  phase: 'mft' | 'walk' | 'index';
}

export interface ScanSummary {
  scanId: string;
  target: string;
  backend: string;
  files: number;
  dirs: number;
  bytes: number;
  elapsedMs: number;
  completed: boolean;
  root: number;
  fallbackReason: string | null;
}

export interface TreeNode {
  id: number;
  name: string;
  path: string;
  size: number;
  isDir: boolean;
  childCount: number;
  children: TreeNode[];
}

export interface FileFilter {
  extensions: string[];
  minSize?: number | null;
  maxSize?: number | null;
  modifiedAfter?: number | null;
  modifiedBefore?: number | null;
  contains?: string | null;
  pathRegex?: string | null;
  under?: number | null;
  includeDirs: boolean;
}

export type SortKey = 'size' | 'name' | 'modified';

export interface FileRow {
  id: number;
  path: string;
  name: string;
  size: number;
  modified: number;
  isDir: boolean;
}

export interface QueryResult {
  rows: FileRow[];
  totalMatches: number;
  totalBytes: number;
}

export type Category = 'files' | 'browser' | 'games' | 'development' | 'aiTools' | 'system' | 'custom';
export type SecretAction = 'chromiumPasswords' | 'windowsVault' | 'gitCredentials' | 'steamSentry';

export interface Profile {
  id: string;
  name: string;
  description: string;
  category: Category;
  include: string[];
  exclude: string[];
  secrets: SecretAction[];
  enabledByDefault: boolean;
  builtin: boolean;
  notes: string[];
}

export interface Detection {
  id: string;
  name: string;
  found: boolean;
  paths: string[];
  approxBytes: number;
  detail: string | null;
}

export interface BrowserProfile {
  browser: string;
  profile: string;
  engine: 'chromium' | 'gecko';
  dataDir: string;
  localState: string | null;
  backupDirs: string[];
  hasLoginDb: boolean;
  appBound: boolean;
  sizeHintBytes: number;
  notes: string[];
}

export interface SteamAccount {
  steamId64: string;
  steamId3: number;
  accountName: string;
  personaName: string;
  rememberPassword: boolean;
  mostRecent: boolean;
  lastLogin: number;
  userdataDir: string | null;
}

export interface SteamReport {
  installDir: string | null;
  accounts: SteamAccount[];
  sentryFiles: string[];
  libraryFolders: string[];
  configFiles: string[];
  warnings: string[];
}

export interface Remote { name: string; url: string; scheme: string }

export interface RepoInfo {
  path: string;
  branch: string | null;
  remotes: Remote[];
  credentialHelper: string | null;
  dirty: boolean | null;
  ahead: number | null;
  untracked: number | null;
  bare: boolean;
  worktreeBytes: number;
  mustBackUp: boolean;
  notes: string[];
}

export interface GitReport {
  repos: RepoInfo[];
  globalCredentialHelper: string | null;
  gitCredentialsFile: string | null;
  globalConfig: string | null;
  sshKeys: string[];
  warnings: string[];
}

export interface CredentialInfo {
  target: string;
  username: string;
  kind: string;
  persist: string;
  lastWritten: number;
  blobBytes: number;
}

export interface VaultInfo {
  credentials: CredentialInfo[];
  steps: string[];
  supported: boolean;
  message: string | null;
}

export type ArchiveMode = 'none' | 'zip' | 'sevenZip';

export interface BackupSelection {
  profileIds: string[];
  extraPaths: string[];
  excludedPaths: string[];
  customIncludes: string[];
  archive: ArchiveMode;
  outputDir?: string | null;
  runGitStatus: boolean;
}

export interface PlanItem {
  source: string;
  staged: string;
  bytes: number;
  modified: number;
  profile: string;
}

export interface SkippedItem { path: string; reason: string }

export interface BackupPlan {
  id: string;
  staging: string;
  items: PlanItem[];
  totalBytes: number;
  fileCount: number;
  secretActions: string[];
  skipped: SkippedItem[];
  warnings: string[];
  archive: ArchiveMode;
  freeBytes: number;
}

export interface BackupProgress {
  phase: 'stage' | 'secrets' | 'archive' | 'verify';
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
  current: string;
}

export interface LogLine { level: 'info' | 'warn' | 'error'; message: string }

export interface ArchiveInfo {
  path: string;
  format: string;
  bytes: number;
  sha256: string;
  encrypted: boolean;
  cipher: string | null;
}

export interface VerifyResult {
  checked: number;
  ok: number;
  mismatched: string[];
  missing: string[];
  archiveOk: boolean | null;
}

export interface SealedArtifact {
  path: string;
  label: string;
  source: string;
  items: number;
  bytes: number;
  sha256: string;
  encrypted: boolean;
  algorithm: string;
  kdf: string;
}

export interface BackupResult {
  planId: string;
  staging: string;
  manifestPath: string;
  reportPath: string;
  restoreScript: string;
  archive: ArchiveInfo | null;
  verify: VerifyResult;
  files: number;
  bytes: number;
  sealed: number;
  elapsedMs: number;
  warnings: string[];
  succeeded: boolean;
}

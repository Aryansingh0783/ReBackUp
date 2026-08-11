import { useMemo, useState } from 'react';
import { AlertTriangle, Eye, EyeOff, FolderOpen, Loader2, Play, ShieldCheck } from 'lucide-react';
import clsx from 'clsx';
import { open } from '@tauri-apps/plugin-dialog';
import { useBackup } from '@/stores/backupStore';
import { useScan } from '@/stores/scanStore';
import { api } from '@/lib/api';
import { bytes, shortPath } from '@/lib/format';
import ProgressModal from './ProgressModal';
import type { ArchiveMode } from '@/types';

const ARCHIVES: Array<{ id: ArchiveMode; label: string; hint: string }> = [
  { id: 'sevenZip', label: '7z + AES-256', hint: 'Smallest and encrypted. Slowest to build.' },
  { id: 'zip', label: 'zip + zstd', hint: 'Fast and portable. NOT encrypted.' },
  { id: 'none', label: 'Folder only', hint: 'No archive — copy the staging folder yourself.' },
];

export default function ReviewView() {
  const b = useBackup();
  const selected = useScan((s) => s.selected);
  const extraPaths = useMemo(() => [...selected], [selected]);

  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');
  const [show, setShow] = useState(false);
  const [passErr, setPassErr] = useState<string | null>(null);

  const needsPassphrase = b.plan
    ? b.plan.secretActions.length > 0 || b.archive === 'sevenZip'
    : true;
  const mismatch = confirm.length > 0 && passphrase !== confirm;
  const canRun =
    !!b.plan &&
    !b.running &&
    (!needsPassphrase || (passphrase.length > 0 && !mismatch && !passErr));

  const byProfile = useMemo(() => {
    const m = new Map<string, { files: number; bytes: number }>();
    for (const i of b.plan?.items ?? []) {
      const cur = m.get(i.profile) ?? { files: 0, bytes: 0 };
      cur.files += 1;
      cur.bytes += i.bytes;
      m.set(i.profile, cur);
    }
    return [...m.entries()].sort((x, y) => y[1].bytes - x[1].bytes);
  }, [b.plan]);

  return (
    <div className="h-full overflow-auto px-6 py-5">
      <ProgressModal />

      <header className="mb-5">
        <h1 className="text-lg font-semibold">Review & run</h1>
        <p className="text-sm text-slate-500">
          {b.enabled.size} profile(s) selected
          {extraPaths.length > 0 && ` + ${extraPaths.length} hand-picked item(s) from the scanner`}.
        </p>
      </header>

      <div className="grid grid-cols-1 xl:grid-cols-[1.4fr_1fr] gap-5 max-w-6xl">
        <div className="space-y-4">
          <section className="panel p-4">
            <div className="flex items-center gap-3 mb-3">
              <h2 className="text-sm font-semibold">Destination</h2>
              <button
                className="btn-ghost text-xs inline-flex items-center gap-1.5 ml-auto"
                onClick={async () => {
                  const dir = await open({ directory: true, multiple: false });
                  if (typeof dir === 'string') b.setOutputDir(dir);
                }}
              >
                <FolderOpen className="w-3.5 h-3.5" />
                Choose folder
              </button>
            </div>
            <div className="font-mono text-xs text-slate-400 selectable">
              {b.outputDir ?? '%TEMP% (default)'}
            </div>
            <p className="text-[11px] text-slate-600 mt-1.5">
              Pick the external drive you'll keep after the reset. Staging plus an archive can use roughly
              twice the source size.
            </p>

            <div className="mt-4 grid grid-cols-3 gap-2">
              {ARCHIVES.map((a) => (
                <button
                  key={a.id}
                  onClick={() => b.setArchive(a.id)}
                  className={clsx(
                    'panel p-2.5 text-left transition-colors',
                    b.archive === a.id ? 'border-accent' : 'hover:border-base-500',
                  )}
                >
                  <div className="text-xs font-medium">{a.label}</div>
                  <div className="text-[10px] text-slate-500 mt-0.5 leading-snug">{a.hint}</div>
                </button>
              ))}
            </div>

            <label className="mt-3 flex items-center gap-2 text-xs text-slate-400 cursor-pointer">
              <input
                type="checkbox"
                checked={b.runGitStatus}
                onChange={(e) => b.setRunGitStatus(e.target.checked)}
                className="accent-[#4f9cf9]"
              />
              Run <code className="text-slate-300">git status</code> in every repo (slower, but finds
              uncommitted work)
            </label>
          </section>

          <section className="panel p-4">
            <div className="flex items-center gap-3 mb-3">
              <h2 className="text-sm font-semibold">Plan</h2>
              <button
                className="btn-ghost text-xs ml-auto"
                disabled={b.planning}
                onClick={() => void b.buildPlan(extraPaths)}
              >
                {b.planning ? (
                  <span className="inline-flex items-center gap-1.5">
                    <Loader2 className="w-3.5 h-3.5 animate-spin" /> Measuring…
                  </span>
                ) : b.plan ? (
                  'Recalculate'
                ) : (
                  'Calculate'
                )}
              </button>
            </div>

            {!b.plan ? (
              <p className="text-xs text-slate-600">
                Calculate to resolve every glob and get the exact size before anything is written.
              </p>
            ) : (
              <>
                <div className="grid grid-cols-3 gap-3 mb-3">
                  <div>
                    <div className="label">Files</div>
                    <div className="text-xl font-semibold tabular-nums">
                      {b.plan.fileCount.toLocaleString()}
                    </div>
                  </div>
                  <div>
                    <div className="label">Size</div>
                    <div className="text-xl font-semibold tabular-nums">{bytes(b.plan.totalBytes)}</div>
                  </div>
                  <div>
                    <div className="label">Free at destination</div>
                    <div
                      className={clsx(
                        'text-xl font-semibold tabular-nums',
                        b.plan.freeBytes > 0 && b.plan.totalBytes * 2 > b.plan.freeBytes && 'text-warn',
                      )}
                    >
                      {bytes(b.plan.freeBytes)}
                    </div>
                  </div>
                </div>

                <div className="space-y-1">
                  {byProfile.map(([name, v]) => (
                    <div key={name} className="flex items-center gap-3 text-xs">
                      <span className="w-40 shrink-0 text-slate-300">{name}</span>
                      <div className="flex-1 h-1.5 rounded-full bg-base-900 overflow-hidden">
                        <div
                          className="h-full bg-accent/70 rounded-full"
                          style={{ width: `${(v.bytes / Math.max(1, b.plan!.totalBytes)) * 100}%` }}
                        />
                      </div>
                      <span className="w-20 text-right text-slate-500 tabular-nums">{bytes(v.bytes)}</span>
                      <span className="w-16 text-right text-slate-600 tabular-nums">
                        {v.files.toLocaleString()}
                      </span>
                    </div>
                  ))}
                </div>

                {b.plan.skipped.length > 0 && (
                  <details className="mt-3">
                    <summary className="text-xs text-slate-500 cursor-pointer hover:text-slate-300">
                      {b.plan.skipped.length} path(s) skipped
                    </summary>
                    <div className="mt-1.5 max-h-32 overflow-auto space-y-0.5">
                      {b.plan.skipped.slice(0, 100).map((s, i) => (
                        <div key={i} className="text-[11px] text-slate-600 font-mono">
                          {shortPath(s.path, 60)} — {s.reason}
                        </div>
                      ))}
                    </div>
                  </details>
                )}
              </>
            )}
          </section>

          {(b.plan?.warnings.length ?? 0) > 0 && (
            <section className="panel p-4 border-warn/30">
              <h2 className="text-sm font-semibold mb-2 flex items-center gap-2">
                <AlertTriangle className="w-4 h-4 text-warn" /> Read this before running
              </h2>
              <div className="space-y-1.5 max-h-64 overflow-auto">
                {b.plan!.warnings.map((w, i) => (
                  <p key={i} className="text-[11px] text-slate-400 leading-relaxed">— {w}</p>
                ))}
              </div>
            </section>
          )}
        </div>

        <div className="space-y-4">
          <section className="panel p-4">
            <h2 className="text-sm font-semibold mb-1 flex items-center gap-2">
              <ShieldCheck className="w-4 h-4 text-accent" /> Passphrase
            </h2>
            <p className="text-[11px] text-slate-500 leading-relaxed mb-3">
              Used to derive an Argon2id key that seals every secret and (with 7z) the archive itself.
              <b className="text-slate-300"> There is no recovery.</b> Write it down somewhere that isn't
              this machine.
            </p>

            <div className="relative">
              <input
                type={show ? 'text' : 'password'}
                className="field w-full pr-9"
                placeholder="at least 12 characters"
                value={passphrase}
                autoComplete="new-password"
                onChange={(e) => {
                  setPassphrase(e.target.value);
                  setPassErr(null);
                }}
                onBlur={async () => {
                  if (!passphrase) return;
                  try {
                    await api.checkPassphrase(passphrase);
                    setPassErr(null);
                  } catch (e) {
                    setPassErr((e as Error).message);
                  }
                }}
              />
              <button
                className="absolute right-2 top-2 text-slate-600 hover:text-slate-300"
                onClick={() => setShow(!show)}
                tabIndex={-1}
              >
                {show ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>

            <input
              type={show ? 'text' : 'password'}
              className="field w-full mt-2"
              placeholder="confirm"
              value={confirm}
              autoComplete="new-password"
              onChange={(e) => setConfirm(e.target.value)}
            />

            {passErr && <p className="text-[11px] text-danger mt-1.5">{passErr}</p>}
            {mismatch && <p className="text-[11px] text-danger mt-1.5">Passphrases don't match.</p>}
            {!needsPassphrase && (
              <p className="text-[11px] text-slate-600 mt-1.5">
                Nothing in this selection needs encryption, so the passphrase is optional.
              </p>
            )}
          </section>

          {(b.plan?.secretActions.length ?? 0) > 0 && (
            <section className="panel p-4">
              <h2 className="text-sm font-semibold mb-2">Secret handling</h2>
              <div className="space-y-1.5">
                {b.plan!.secretActions.map((a) => (
                  <div key={a} className="text-[11px] text-slate-400">
                    <span className="chip bg-accent/15 text-accent mr-2">{a}</span>
                    sealed with AES-256-GCM, never written in the clear
                  </div>
                ))}
              </div>
            </section>
          )}

          <button
            className="btn-primary w-full py-3 inline-flex items-center justify-center gap-2"
            disabled={!canRun}
            onClick={() => void b.run(extraPaths, passphrase)}
          >
            <Play className="w-4 h-4" />
            Run backup
          </button>

          {b.error && (
            <div className="panel p-3 border-danger/40 text-xs text-danger selectable">{b.error}</div>
          )}
        </div>
      </div>
    </div>
  );
}

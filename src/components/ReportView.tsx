import { CheckCircle2, ExternalLink, FileText, ShieldAlert, ShieldCheck, Terminal } from 'lucide-react';
import { openPath } from '@tauri-apps/plugin-opener';
import { useBackup } from '@/stores/backupStore';
import { bytes, duration } from '@/lib/format';

export default function ReportView() {
  const { result, reset } = useBackup();

  if (!result) {
    return (
      <div className="h-full flex items-center justify-center text-sm text-slate-600">
        No backup has finished yet.
      </div>
    );
  }

  const ok = result.succeeded;

  return (
    <div className="h-full overflow-auto px-6 py-5">
      <div className="max-w-3xl">
        <div className="flex items-start gap-3 mb-5">
          {ok ? (
            <ShieldCheck className="w-8 h-8 text-ok shrink-0" />
          ) : (
            <ShieldAlert className="w-8 h-8 text-danger shrink-0" />
          )}
          <div>
            <h1 className="text-lg font-semibold">
              {ok ? 'Backup complete and verified' : 'Backup finished with problems'}
            </h1>
            <p className="text-sm text-slate-500">
              {result.files.toLocaleString()} files · {bytes(result.bytes)} · {result.sealed} sealed
              artifact(s) · {duration(result.elapsedMs)}
            </p>
          </div>
        </div>

        {!ok && (
          <div className="panel p-4 border-danger/40 mb-5">
            <h2 className="text-sm font-semibold text-danger mb-2">Do not reset yet</h2>
            <p className="text-xs text-slate-400 mb-2">
              {result.verify.mismatched.length} file(s) didn't match their recorded hash and{' '}
              {result.verify.missing.length} went missing. Re-run the backup, and if it repeats, check the
              destination drive for faults.
            </p>
            <div className="max-h-40 overflow-auto space-y-0.5">
              {[...result.verify.mismatched, ...result.verify.missing].slice(0, 40).map((p) => (
                <div key={p} className="font-mono text-[11px] text-slate-500 selectable">{p}</div>
              ))}
            </div>
          </div>
        )}

        <section className="panel p-4 mb-4">
          <h2 className="text-sm font-semibold mb-3">Where everything is</h2>
          <div className="space-y-2">
            {[
              { label: 'Staging folder', value: result.staging, open: result.staging },
              { label: 'HTML report', value: result.reportPath, open: result.reportPath },
              { label: 'Manifest', value: result.manifestPath, open: result.manifestPath },
              { label: 'Restore script', value: result.restoreScript, open: result.staging },
              ...(result.archive
                ? [{ label: 'Archive', value: result.archive.path, open: result.archive.path }]
                : []),
            ].map((row) => (
              <div key={row.label} className="flex items-center gap-3 text-xs">
                <span className="w-28 shrink-0 text-slate-500">{row.label}</span>
                <span className="font-mono text-slate-300 truncate flex-1 selectable">{row.value}</span>
                <button
                  className="btn-ghost text-[11px] py-1 shrink-0 inline-flex items-center gap-1"
                  onClick={() => void openPath(row.open)}
                >
                  <ExternalLink className="w-3 h-3" /> Open
                </button>
              </div>
            ))}
          </div>
          {result.archive && (
            <p className="text-[11px] text-slate-500 mt-3">
              {result.archive.format} · {bytes(result.archive.bytes)} ·{' '}
              {result.archive.encrypted ? (
                <span className="text-ok">encrypted ({result.archive.cipher})</span>
              ) : (
                <span className="text-warn">
                  not encrypted — the sealed artifacts inside are still encrypted
                </span>
              )}
            </p>
          )}
        </section>

        <section className="panel p-4 mb-4">
          <h2 className="text-sm font-semibold mb-3 flex items-center gap-2">
            <CheckCircle2 className="w-4 h-4 text-ok" /> Do these now, in order
          </h2>
          <ol className="space-y-2 text-xs text-slate-400 list-decimal list-inside leading-relaxed">
            <li>
              Copy the <b className="text-slate-200">entire staging folder</b> (or the archive) to an
              external drive. Nothing on this machine survives the reset.
            </li>
            <li>
              Open the HTML report and skim the warnings — especially the Steam and Credential Manager ones.
            </li>
            <li>
              Verify from the copy, not the original:{' '}
              <code className="text-slate-300">pre-reset-backup.exe verify --manifest manifest.json</code>
            </li>
            <li>Write your passphrase down somewhere that isn't this computer.</li>
            <li>Only then, reset Windows.</li>
          </ol>
        </section>

        <section className="panel p-4 mb-4">
          <h2 className="text-sm font-semibold mb-3 flex items-center gap-2">
            <Terminal className="w-4 h-4" /> After the clean install
          </h2>
          <pre className="bg-base-900 rounded-lg p-3 text-[11px] font-mono text-slate-400 overflow-auto selectable">
{`# 1. Restore files (idempotent — safe to re-run)
.\\restore.cmd            # or: powershell -File restore.ps1 -DryRun

# 2. Unseal the password CSV
pre-reset-backup.exe unseal --in secrets\\opera-gx-default-passwords.csv.prb --out passwords.csv

# 3. Import it in the browser's password manager, then:
pre-reset-backup.exe shred --path passwords.csv`}
          </pre>
        </section>

        {result.warnings.length > 0 && (
          <section className="panel p-4 mb-4">
            <h2 className="text-sm font-semibold mb-2 flex items-center gap-2">
              <FileText className="w-4 h-4 text-warn" /> Warnings recorded in the manifest
            </h2>
            <div className="space-y-1.5 max-h-72 overflow-auto">
              {result.warnings.map((w, i) => (
                <p key={i} className="text-[11px] text-slate-400 leading-relaxed">— {w}</p>
              ))}
            </div>
          </section>
        )}

        <button className="btn-ghost" onClick={reset}>
          Start another backup
        </button>
      </div>
    </div>
  );
}

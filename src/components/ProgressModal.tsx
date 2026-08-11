import { useEffect, useRef } from 'react';
import { Loader2, Square } from 'lucide-react';
import clsx from 'clsx';
import { useBackup } from '@/stores/backupStore';
import { bytes, shortPath } from '@/lib/format';

const PHASES = [
  { id: 'stage', label: 'Copying files' },
  { id: 'secrets', label: 'Sealing secrets' },
  { id: 'archive', label: 'Compressing' },
  { id: 'verify', label: 'Verifying hashes' },
] as const;

export default function ProgressModal() {
  const { running, progress, log, cancel } = useBackup();
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [log]);

  if (!running) return null;

  const pct = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;
  const activeIndex = PHASES.findIndex((p) => p.id === progress?.phase);

  return (
    <div className="fixed inset-0 z-50 bg-base-900/85 backdrop-blur-sm flex items-center justify-center p-8">
      <div className="panel w-full max-w-2xl p-6">
        <div className="flex items-center gap-2 mb-5">
          <Loader2 className="w-5 h-5 animate-spin text-accent" />
          <h2 className="text-base font-semibold">Backing up</h2>
          <button className="btn-danger ml-auto inline-flex items-center gap-1.5" onClick={() => void cancel()}>
            <Square className="w-3.5 h-3.5" />
            Cancel
          </button>
        </div>

        <div className="flex gap-1 mb-4">
          {PHASES.map((p, i) => (
            <div key={p.id} className="flex-1">
              <div
                className={clsx(
                  'h-1 rounded-full',
                  i < activeIndex ? 'bg-ok' : i === activeIndex ? 'bg-accent' : 'bg-base-600',
                )}
              />
              <div
                className={clsx(
                  'text-[10px] mt-1.5',
                  i === activeIndex ? 'text-accent' : i < activeIndex ? 'text-ok' : 'text-slate-600',
                )}
              >
                {p.label}
              </div>
            </div>
          ))}
        </div>

        {progress && (
          <>
            <div className="h-2 rounded-full bg-base-900 overflow-hidden">
              <div
                className="h-full bg-accent transition-[width] duration-150"
                style={{ width: `${Math.min(100, pct)}%` }}
              />
            </div>
            <div className="flex justify-between text-xs text-slate-500 mt-1.5 tabular-nums">
              <span>
                {progress.done.toLocaleString()} / {progress.total.toLocaleString()}
              </span>
              <span>
                {bytes(progress.bytesDone)} / {bytes(progress.bytesTotal)}
              </span>
            </div>
            {progress.current && (
              <div className="mt-1 font-mono text-[11px] text-slate-600 truncate">
                {shortPath(progress.current, 80)}
              </div>
            )}
          </>
        )}

        <div
          ref={logRef}
          className="mt-4 h-40 overflow-auto bg-base-900 rounded-lg border border-base-600 p-2.5 font-mono text-[11px] space-y-0.5"
        >
          {log.length === 0 ? (
            <div className="text-slate-700">waiting for the first log line…</div>
          ) : (
            log.map((l, i) => (
              <div
                key={i}
                className={clsx(
                  'selectable',
                  l.level === 'error' ? 'text-danger' : l.level === 'warn' ? 'text-warn' : 'text-slate-500',
                )}
              >
                {l.message}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

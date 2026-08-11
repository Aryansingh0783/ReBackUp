import { useEffect, useState } from 'react';
import { ChevronRight, Loader2, Pause, Play, Search, Square, Zap } from 'lucide-react';
import clsx from 'clsx';
import { useSettings } from '@/stores/settingsStore';
import { useScan } from '@/stores/scanStore';
import { useBackup } from '@/stores/backupStore';
import DriveSelector from './DriveSelector';
import TreemapView from './TreemapView';
import FileTable from './FileTable';
import { bytes, count, duration } from '@/lib/format';
import type { ScanBackend } from '@/types';

export default function ScannerView() {
  const { drives, env } = useSettings();
  const scan = useScan();
  const setExcluded = useBackup((s) => s.setExcluded);

  const [target, setTarget] = useState<string | null>(null);
  const [backend, setBackend] = useState<ScanBackend>('auto');
  const [search, setSearch] = useState('');
  const [minMb, setMinMb] = useState('');
  const [exts, setExts] = useState('');
  const [regex, setRegex] = useState('');

  useEffect(() => {
    if (!target && drives.length > 0) setTarget(drives[0].mount);
  }, [drives, target]);

  // Debounce filter edits — every keystroke otherwise re-queries a 3M row index.
  useEffect(() => {
    const t = setTimeout(() => {
      void scan.setFilter({
        contains: search || null,
        minSize: minMb ? Math.round(Number(minMb) * 1024 * 1024) : null,
        extensions: exts.split(',').map((s) => s.trim()).filter(Boolean),
        pathRegex: regex || null,
      });
    }, 250);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search, minMb, exts, regex]);

  const pct =
    scan.progress && scan.progress.total > 0
      ? (scan.progress.done / scan.progress.total) * 100
      : null;

  return (
    <div className="h-full flex flex-col overflow-hidden">
      <header className="px-6 pt-5 pb-3 shrink-0">
        <h1 className="text-lg font-semibold">Scanner</h1>
        <p className="text-sm text-slate-500">
          Find what's actually taking space, then tick the files worth keeping.
        </p>
      </header>

      <div className="px-6 pb-3 shrink-0">
        <DriveSelector
          drives={drives}
          value={target}
          onChange={setTarget}
          mftAvailable={env?.mftAvailable ?? false}
        />
      </div>

      <div className="px-6 pb-3 flex flex-wrap items-center gap-2 shrink-0">
        <select
          value={backend}
          onChange={(e) => setBackend(e.target.value as ScanBackend)}
          className="field"
          title="MFT reads the NTFS index directly (needs elevation). Walk works everywhere."
        >
          <option value="auto">Auto (MFT, fall back to walk)</option>
          <option value="mft">Force MFT</option>
          <option value="walk">Force directory walk</option>
        </select>

        {!scan.running ? (
          <button
            className="btn-primary inline-flex items-center gap-1.5"
            disabled={!target}
            onClick={() => target && void scan.start({ target, backend })}
          >
            <Zap className="w-4 h-4" />
            Scan {target}
          </button>
        ) : (
          <>
            <button className="btn-ghost inline-flex items-center gap-1.5" onClick={() => void scan.togglePause()}>
              {scan.paused ? <Play className="w-4 h-4" /> : <Pause className="w-4 h-4" />}
              {scan.paused ? 'Resume' : 'Pause'}
            </button>
            <button className="btn-danger inline-flex items-center gap-1.5" onClick={() => void scan.cancel()}>
              <Square className="w-4 h-4" />
              Cancel
            </button>
            <span className="inline-flex items-center gap-2 text-sm text-slate-400">
              <Loader2 className="w-4 h-4 animate-spin" />
              {scan.progress?.phase ?? 'starting'}
              {pct !== null && ` — ${pct.toFixed(0)}%`}
              {scan.progress && ` (${count(scan.progress.done)} records)`}
            </span>
          </>
        )}

        {scan.summary && !scan.running && (
          <span className="text-sm text-slate-400 ml-auto">
            {count(scan.summary.files)} files · {count(scan.summary.dirs)} dirs ·{' '}
            {bytes(scan.summary.bytes)} · {duration(scan.summary.elapsedMs)} ·{' '}
            <span className={scan.summary.backend === 'mft' ? 'text-accent' : 'text-slate-500'}>
              {scan.summary.backend}
            </span>
          </span>
        )}
      </div>

      {scan.summary?.fallbackReason && (
        <div className="mx-6 mb-3 p-2.5 rounded-lg bg-warn/10 border border-warn/30 text-xs text-warn shrink-0">
          Fell back to the slow scanner: {scan.summary.fallbackReason}
        </div>
      )}
      {scan.error && (
        <div className="mx-6 mb-3 p-2.5 rounded-lg bg-danger/10 border border-danger/30 text-xs text-danger shrink-0">
          {scan.error}
        </div>
      )}

      {scan.crumbs.length > 0 && (
        <div className="px-6 pb-2 flex items-center gap-1 text-xs text-slate-400 shrink-0 flex-wrap">
          {scan.crumbs.map((c, i) => (
            <span key={`${c.id}-${i}`} className="inline-flex items-center gap-1">
              {i > 0 && <ChevronRight className="w-3 h-3 text-slate-600" />}
              <button
                className={clsx('hover:text-accent', i === scan.crumbs.length - 1 && 'text-slate-200')}
                onClick={() => void scan.crumbTo(i)}
              >
                {c.name || '\\'}
              </button>
            </span>
          ))}
        </div>
      )}

      <div className="flex-1 min-h-0 px-6 pb-6 grid grid-rows-[auto_1fr] gap-4 overflow-hidden">
        <TreemapView root={scan.tree} onDrill={(id) => void scan.drill(id)} height={300} />

        <div className="min-h-0 flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative">
              <Search className="w-3.5 h-3.5 absolute left-2.5 top-2.5 text-slate-600" />
              <input
                className="field pl-8 w-64"
                placeholder="path contains…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            <input
              className="field w-40"
              placeholder="ext: psd, mp4, zip"
              value={exts}
              onChange={(e) => setExts(e.target.value)}
            />
            <input
              className="field w-28"
              placeholder="min MB"
              inputMode="decimal"
              value={minMb}
              onChange={(e) => setMinMb(e.target.value)}
            />
            <input
              className="field w-56 font-mono text-xs"
              placeholder="regex, e.g. \\.(kdbx|env)$"
              value={regex}
              onChange={(e) => setRegex(e.target.value)}
            />
            <span className="text-xs text-slate-500 ml-auto">
              {count(scan.totalMatches)} matches · {bytes(scan.totalBytes)}
            </span>
            <button className="btn-ghost text-xs" onClick={scan.selectAllVisible}>
              Select all shown
            </button>
            <button
              className="btn-ghost text-xs"
              onClick={() => {
                scan.clearSelection();
                setExcluded([]);
              }}
            >
              Clear
            </button>
          </div>

          <div className="flex-1 min-h-0">
            <FileTable
              rows={scan.rows}
              selected={scan.selected}
              sort={scan.sort}
              desc={scan.desc}
              totalMatches={scan.totalMatches}
              onSort={(k) => void scan.setSort(k, k === scan.sort ? !scan.desc : true)}
              onToggle={scan.toggleSelect}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

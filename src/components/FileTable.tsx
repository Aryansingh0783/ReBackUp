/**
 * Virtualised file list. A scan can match hundreds of thousands of rows, so
 * only the visible window is mounted.
 */
import { useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ArrowDown, ArrowUp, Check, Folder, File as FileIcon } from 'lucide-react';
import clsx from 'clsx';
import { bytes, when, shortPath } from '@/lib/format';
import type { FileRow, SortKey } from '@/types';

interface Props {
  rows: FileRow[];
  selected: Set<string>;
  sort: SortKey;
  desc: boolean;
  onSort: (key: SortKey) => void;
  onToggle: (path: string) => void;
  totalMatches: number;
}

export default function FileTable({
  rows, selected, sort, desc, onSort, onToggle, totalMatches,
}: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 30,
    overscan: 16,
  });

  const Header = ({ label, k, className }: { label: string; k?: SortKey; className?: string }) => (
    <div
      className={clsx('label px-3 py-2', k && 'cursor-pointer hover:text-slate-300', className)}
      onClick={k ? () => onSort(k) : undefined}
    >
      {label}
      {k === sort && (desc ? <ArrowDown className="inline w-3 h-3 ml-1" /> : <ArrowUp className="inline w-3 h-3 ml-1" />)}
    </div>
  );

  return (
    <div className="panel flex flex-col overflow-hidden h-full">
      <div className="grid grid-cols-[32px_1fr_110px_110px] border-b border-base-600 shrink-0">
        <div />
        <Header label="Path" k="name" />
        <Header label="Size" k="size" className="text-right" />
        <Header label="Modified" k="modified" className="text-right" />
      </div>

      <div ref={parentRef} className="flex-1 overflow-auto">
        {rows.length === 0 ? (
          <div className="p-8 text-center text-sm text-slate-600">
            No files match the current filter.
          </div>
        ) : (
          <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
            {virt.getVirtualItems().map((v) => {
              const r = rows[v.index];
              const isSel = selected.has(r.path);
              return (
                <div
                  key={r.id}
                  onClick={() => onToggle(r.path)}
                  className={clsx(
                    'absolute left-0 w-full grid grid-cols-[32px_1fr_110px_110px] items-center',
                    'text-[13px] cursor-pointer border-b border-base-700/50',
                    isSel ? 'bg-accent/10' : 'hover:bg-base-700/40',
                  )}
                  style={{ height: v.size, transform: `translateY(${v.start}px)` }}
                >
                  <div className="flex justify-center">
                    <div
                      className={clsx(
                        'w-4 h-4 rounded border flex items-center justify-center',
                        isSel ? 'bg-accent border-accent' : 'border-base-500',
                      )}
                    >
                      {isSel && <Check className="w-3 h-3 text-base-900" />}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 min-w-0 px-1">
                    {r.isDir ? (
                      <Folder className="w-3.5 h-3.5 shrink-0 text-slate-500" />
                    ) : (
                      <FileIcon className="w-3.5 h-3.5 shrink-0 text-slate-600" />
                    )}
                    <span className="truncate font-mono text-[12px] selectable" title={r.path}>
                      {shortPath(r.path, 90)}
                    </span>
                  </div>
                  <div className="text-right px-3 tabular-nums text-slate-300">{bytes(r.size)}</div>
                  <div className="text-right px-3 tabular-nums text-slate-500">{when(r.modified)}</div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="border-t border-base-600 px-3 py-1.5 text-[11px] text-slate-500 shrink-0">
        showing {rows.length.toLocaleString()} of {totalMatches.toLocaleString()} matches
        {totalMatches > rows.length && ' — narrow the filter to see the rest'}
      </div>
    </div>
  );
}

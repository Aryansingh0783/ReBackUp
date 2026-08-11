import { HardDrive, Usb, Zap, FolderSearch } from 'lucide-react';
import clsx from 'clsx';
import { bytes } from '@/lib/format';
import type { DriveInfo } from '@/types';

interface Props {
  drives: DriveInfo[];
  value: string | null;
  onChange: (mount: string) => void;
  mftAvailable: boolean;
}

export default function DriveSelector({ drives, value, onChange, mftAvailable }: Props) {
  if (drives.length === 0) {
    return (
      <div className="panel p-6 text-center text-sm text-slate-500">
        <FolderSearch className="w-6 h-6 mx-auto mb-2 opacity-50" />
        No drives detected.
      </div>
    );
  }

  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
      {drives.map((d) => {
        const used = d.totalBytes - d.freeBytes;
        const pct = d.totalBytes > 0 ? (used / d.totalBytes) * 100 : 0;
        const active = value === d.mount;
        const fast = d.ntfs && mftAvailable;

        return (
          <button
            key={d.mount}
            onClick={() => onChange(d.mount)}
            className={clsx(
              'panel p-4 text-left transition-all',
              active ? 'border-accent ring-1 ring-accent/40' : 'hover:border-base-500',
            )}
          >
            <div className="flex items-center gap-2 mb-3">
              {d.removable ? (
                <Usb className="w-4 h-4 text-slate-400" />
              ) : (
                <HardDrive className="w-4 h-4 text-slate-400" />
              )}
              <span className="font-semibold text-sm">{d.mount}</span>
              <span className="text-[11px] text-slate-500">{d.fileSystem}</span>
              {fast && (
                <span
                  className="ml-auto chip bg-accent/15 text-accent inline-flex items-center gap-1"
                  title="NTFS + elevated: the raw-MFT scanner can be used"
                >
                  <Zap className="w-3 h-3" />
                  MFT
                </span>
              )}
            </div>

            <div className="h-1.5 rounded-full bg-base-900 overflow-hidden">
              <div
                className={clsx(
                  'h-full rounded-full',
                  pct > 90 ? 'bg-danger' : pct > 75 ? 'bg-warn' : 'bg-accent',
                )}
                style={{ width: `${Math.min(100, pct)}%` }}
              />
            </div>

            <div className="mt-2 flex justify-between text-[11px] text-slate-500">
              <span>{bytes(used)} used</span>
              <span>{bytes(d.freeBytes)} free</span>
            </div>
          </button>
        );
      })}
    </div>
  );
}

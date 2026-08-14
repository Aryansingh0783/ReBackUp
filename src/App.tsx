import { useEffect, useState } from 'react';
import {
  HardDrive, LayoutGrid, ListChecks, Play, ShieldCheck, TriangleAlert,
} from 'lucide-react';
import clsx from 'clsx';
import { useSettings } from '@/stores/settingsStore';
import { useBackup } from '@/stores/backupStore';
import { useScan } from '@/stores/scanStore';
import Dashboard from '@/components/Dashboard';
import ScannerView from '@/components/ScannerView';
import ProfileWizard from '@/components/ProfileWizard';
import ReviewView from '@/components/ReviewView';
import ReportView from '@/components/ReportView';

type Tab = 'dashboard' | 'scanner' | 'profiles' | 'review' | 'report';

const TABS: Array<{ id: Tab; label: string; icon: typeof HardDrive }> = [
  { id: 'dashboard', label: 'Overview', icon: LayoutGrid },
  { id: 'scanner', label: 'Scanner', icon: HardDrive },
  { id: 'profiles', label: 'What to keep', icon: ListChecks },
  { id: 'review', label: 'Review & run', icon: Play },
  { id: 'report', label: 'Result', icon: ShieldCheck },
];

export default function App() {
  const [tab, setTab] = useState<Tab>('dashboard');
  const { env, load } = useSettings();
  const loadProfiles = useBackup((s) => s.loadProfiles);
  const result = useBackup((s) => s.result);
  const enabled = useBackup((s) => s.enabled);
  const selected = useScan((s) => s.selected);

  useEffect(() => {
    void load();
    void loadProfiles();
  }, [load, loadProfiles]);

  // Jump to the result the moment a run finishes.
  useEffect(() => {
    if (result) setTab('report');
  }, [result]);

  return (
    <div className="flex h-full">
      <nav className="w-56 shrink-0 bg-base-800 border-r border-base-600 flex flex-col">
        <div className="px-4 py-4 border-b border-base-600">
          <div className="flex items-center gap-2">
            <ShieldCheck className="w-5 h-5 text-accent" />
            <div>
              <div className="text-sm font-semibold leading-tight">ReBackUp</div>
              <div className="text-[11px] text-slate-500">v{env?.version ?? '…'}</div>
            </div>
          </div>
        </div>

        <div className="flex-1 p-2 space-y-0.5">
          {TABS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => setTab(id)}
              className={clsx(
                'w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-colors',
                tab === id
                  ? 'bg-accent/15 text-accent'
                  : 'text-slate-400 hover:bg-base-700 hover:text-slate-200',
              )}
            >
              <Icon className="w-4 h-4" />
              {label}
              {id === 'profiles' && enabled.size > 0 && (
                <span className="ml-auto chip bg-base-600 text-slate-300">{enabled.size}</span>
              )}
              {id === 'scanner' && selected.size > 0 && (
                <span className="ml-auto chip bg-base-600 text-slate-300">{selected.size}</span>
              )}
            </button>
          ))}
        </div>

        {env && !env.elevated && env.windows && (
          <div className="m-2 p-3 rounded-lg bg-warn/10 border border-warn/30 text-[11px] leading-relaxed text-warn">
            <TriangleAlert className="w-3.5 h-3.5 inline mr-1 -mt-0.5" />
            Not elevated — the MFT fast path is unavailable. Scans fall back to a
            directory walk (much slower on big volumes).
          </div>
        )}

        <div className="p-3 text-[10px] text-slate-600 border-t border-base-600 selectable">
          Local-only. Nothing leaves this machine.
        </div>
      </nav>

      <main className="flex-1 overflow-hidden">
        {tab === 'dashboard' && <Dashboard onNavigate={setTab} />}
        {tab === 'scanner' && <ScannerView />}
        {tab === 'profiles' && <ProfileWizard />}
        {tab === 'review' && <ReviewView />}
        {tab === 'report' && <ReportView />}
      </main>
    </div>
  );
}

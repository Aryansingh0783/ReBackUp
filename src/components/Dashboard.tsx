import { useEffect, useState } from 'react';
import {
  AlertTriangle, ChevronRight, Chrome, Gamepad2, GitBranch, KeyRound, Loader2, ShieldCheck,
} from 'lucide-react';
import { api } from '@/lib/api';
import { useSettings } from '@/stores/settingsStore';
import { bytes } from '@/lib/format';
import type { BrowserProfile, GitReport, SteamReport, VaultInfo } from '@/types';

interface Props {
  onNavigate: (t: 'dashboard' | 'scanner' | 'profiles' | 'review' | 'report') => void;
}

export default function Dashboard({ onNavigate }: Props) {
  const { env, drives } = useSettings();
  const [browsers, setBrowsers] = useState<BrowserProfile[] | null>(null);
  const [steam, setSteam] = useState<SteamReport | null>(null);
  const [git, setGit] = useState<GitReport | null>(null);
  const [vault, setVault] = useState<VaultInfo | null>(null);
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    let live = true;
    (async () => {
      const [b, s, v] = await Promise.allSettled([
        api.detectBrowsers(),
        api.detectSteam(),
        api.credentialManagerInfo(),
      ]);
      if (!live) return;
      if (b.status === 'fulfilled') setBrowsers(b.value);
      if (s.status === 'fulfilled') setSteam(s.value);
      if (v.status === 'fulfilled') setVault(v.value);
      // Git discovery walks the home directory, so it lands after the rest.
      const g = await api.discoverGit([], false).catch(() => null);
      if (live) {
        setGit(g);
        setBusy(false);
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const atRisk = git?.repos.filter((r) => r.mustBackUp) ?? [];
  const totalStorage = drives.reduce((a, d) => a + (d.totalBytes - d.freeBytes), 0);

  const Card = ({
    icon: Icon, title, value, sub, tone = 'default',
  }: {
    icon: typeof Chrome; title: string; value: string; sub?: string;
    tone?: 'default' | 'warn' | 'ok';
  }) => (
    <div className="panel p-4">
      <div className="flex items-center gap-2 mb-2">
        <Icon
          className={
            tone === 'warn' ? 'w-4 h-4 text-warn' : tone === 'ok' ? 'w-4 h-4 text-ok' : 'w-4 h-4 text-slate-400'
          }
        />
        <span className="label">{title}</span>
      </div>
      <div className="text-2xl font-semibold tabular-nums">{value}</div>
      {sub && <div className="text-xs text-slate-500 mt-1">{sub}</div>}
    </div>
  );

  return (
    <div className="h-full overflow-auto px-6 py-5">
      <header className="mb-5">
        <h1 className="text-lg font-semibold">Before you wipe this machine</h1>
        <p className="text-sm text-slate-500">
          {env ? `${env.user}'s account · ${bytes(totalStorage)} in use across ${drives.length} volume(s)` : 'Reading environment…'}
        </p>
      </header>

      {busy && (
        <div className="mb-4 inline-flex items-center gap-2 text-sm text-slate-500">
          <Loader2 className="w-4 h-4 animate-spin" />
          Looking for browsers, Steam, repos and stored credentials…
        </div>
      )}

      <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3 mb-6">
        <Card
          icon={Chrome}
          title="Browser profiles"
          value={String(browsers?.length ?? '—')}
          sub={
            browsers?.some((b) => b.appBound)
              ? 'some use app-bound encryption — manual export needed'
              : browsers?.filter((b) => b.hasLoginDb).length
                ? `${browsers.filter((b) => b.hasLoginDb).length} with saved passwords`
                : undefined
          }
          tone={browsers?.some((b) => b.appBound) ? 'warn' : 'default'}
        />
        <Card
          icon={Gamepad2}
          title="Steam accounts"
          value={String(steam?.accounts.length ?? '—')}
          sub={steam?.sentryFiles.length ? `${steam.sentryFiles.length} sentry file(s)` : 'no sentry files'}
        />
        <Card
          icon={GitBranch}
          title="Repos needing backup"
          value={String(atRisk.length)}
          sub={git ? `${git.repos.length} repo(s) found in your home folder` : undefined}
          tone={atRisk.length > 0 ? 'warn' : 'ok'}
        />
        <Card
          icon={KeyRound}
          title="Stored credentials"
          value={String(vault?.credentials.length ?? '—')}
          sub="inventory only — secrets stay in the vault"
        />
      </div>

      {atRisk.length > 0 && (
        <section className="panel p-4 mb-5 border-warn/40">
          <div className="flex items-center gap-2 mb-3">
            <AlertTriangle className="w-4 h-4 text-warn" />
            <h2 className="text-sm font-semibold">These exist nowhere but this machine</h2>
          </div>
          <div className="space-y-1.5 max-h-56 overflow-auto">
            {atRisk.slice(0, 25).map((r) => (
              <div key={r.path} className="flex items-center gap-3 text-xs">
                <span className="font-mono text-slate-300 truncate flex-1 selectable">{r.path}</span>
                <span className="text-slate-500 shrink-0">{r.branch ?? '—'}</span>
                <span className="chip bg-warn/15 text-warn shrink-0">
                  {r.remotes.length === 0
                    ? 'no remote'
                    : [r.dirty && 'uncommitted', (r.ahead ?? 0) > 0 && 'unpushed', (r.untracked ?? 0) > 0 && 'untracked']
                        .filter(Boolean)
                        .join(', ')}
                </span>
                <span className="text-slate-600 shrink-0 tabular-nums">{bytes(r.worktreeBytes)}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      {browsers && browsers.length > 0 && (
        <section className="panel p-4 mb-5">
          <h2 className="text-sm font-semibold mb-3">Browser profiles</h2>
          <div className="space-y-1.5">
            {browsers.map((b) => (
              <div key={`${b.browser}-${b.profile}-${b.dataDir}`} className="flex items-center gap-3 text-xs">
                <span className="w-32 shrink-0 font-medium">{b.browser}</span>
                <span className="w-20 shrink-0 text-slate-500">{b.profile}</span>
                <span className="font-mono text-slate-500 truncate flex-1 selectable">{b.dataDir}</span>
                {b.appBound ? (
                  <span className="chip bg-warn/15 text-warn shrink-0">manual export</span>
                ) : b.hasLoginDb ? (
                  <span className="chip bg-ok/15 text-ok shrink-0">DPAPI readable</span>
                ) : (
                  <span className="chip bg-base-600 text-slate-400 shrink-0">no passwords</span>
                )}
                <span className="text-slate-600 shrink-0 tabular-nums w-16 text-right">
                  {bytes(b.sizeHintBytes)}
                </span>
              </div>
            ))}
          </div>
        </section>
      )}

      {steam?.warnings.length ? (
        <section className="panel p-4 mb-5 border-warn/30">
          <h2 className="text-sm font-semibold mb-2 flex items-center gap-2">
            <Gamepad2 className="w-4 h-4 text-warn" /> Steam caveats
          </h2>
          {steam.warnings.map((w, i) => (
            <p key={i} className="text-xs text-slate-400 mb-1.5 leading-relaxed">{w}</p>
          ))}
        </section>
      ) : null}

      <div className="flex gap-2">
        <button className="btn-primary inline-flex items-center gap-1.5" onClick={() => onNavigate('profiles')}>
          Choose what to keep <ChevronRight className="w-4 h-4" />
        </button>
        <button className="btn-ghost inline-flex items-center gap-1.5" onClick={() => onNavigate('scanner')}>
          <ShieldCheck className="w-4 h-4" /> Scan a drive first
        </button>
      </div>
    </div>
  );
}

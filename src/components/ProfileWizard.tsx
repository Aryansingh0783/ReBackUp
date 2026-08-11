import { useMemo, useState } from 'react';
import {
  Bot, Check, Chrome, Code2, ExternalLink, FolderOpen, Gamepad2, Info, KeyRound, Plus, Settings2, X,
} from 'lucide-react';
import clsx from 'clsx';
import { useBackup } from '@/stores/backupStore';
import { api } from '@/lib/api';
import { bytes } from '@/lib/format';
import type { Category, Profile } from '@/types';

const ICONS: Record<Category, typeof Chrome> = {
  files: FolderOpen,
  browser: Chrome,
  games: Gamepad2,
  development: Code2,
  aiTools: Bot,
  system: KeyRound,
  custom: Settings2,
};

const GROUPS: Array<{ key: Category; label: string }> = [
  { key: 'files', label: 'Your files' },
  { key: 'browser', label: 'Browsers' },
  { key: 'games', label: 'Games' },
  { key: 'development', label: 'Development' },
  { key: 'aiTools', label: 'AI & editors' },
  { key: 'system', label: 'System & credentials' },
  { key: 'custom', label: 'Custom' },
];

export default function ProfileWizard() {
  const {
    profiles, detections, enabled, toggleProfile, customIncludes, addCustomInclude, removeCustomInclude,
  } = useBackup();
  const [pattern, setPattern] = useState('');
  const [expanded, setExpanded] = useState<string | null>(null);

  const detectionById = useMemo(
    () => new Map(detections.map((d) => [d.id, d])),
    [detections],
  );

  const byGroup = useMemo(() => {
    const m = new Map<Category, Profile[]>();
    for (const p of profiles) {
      const list = m.get(p.category) ?? [];
      list.push(p);
      m.set(p.category, list);
    }
    return m;
  }, [profiles]);

  const estimate = useMemo(
    () =>
      profiles
        .filter((p) => enabled.has(p.id))
        .reduce((a, p) => a + (detectionById.get(p.id)?.approxBytes ?? 0), 0),
    [profiles, enabled, detectionById],
  );

  return (
    <div className="h-full overflow-auto px-6 py-5">
      <header className="mb-5">
        <h1 className="text-lg font-semibold">What should survive the reset?</h1>
        <p className="text-sm text-slate-500">
          Things found on this machine are pre-ticked. Rough estimate:{' '}
          <span className="text-slate-300">{bytes(estimate)}</span> — the review step measures it exactly.
        </p>
      </header>

      <div className="space-y-6 max-w-4xl">
        {GROUPS.map(({ key, label }) => {
          const list = byGroup.get(key);
          if (!list?.length) return null;
          const Icon = ICONS[key];

          return (
            <section key={key}>
              <h2 className="label mb-2 flex items-center gap-1.5">
                <Icon className="w-3.5 h-3.5" /> {label}
              </h2>

              <div className="space-y-2">
                {list.map((p) => {
                  const det = detectionById.get(p.id);
                  const on = enabled.has(p.id);
                  const missing = det && !det.found && p.include.length > 0;

                  return (
                    <div
                      key={p.id}
                      className={clsx(
                        'panel p-3 transition-colors',
                        on && 'border-accent/50',
                        missing && 'opacity-55',
                      )}
                    >
                      <div className="flex items-start gap-3">
                        <button
                          onClick={() => toggleProfile(p.id)}
                          className={clsx(
                            'mt-0.5 w-4 h-4 rounded border shrink-0 flex items-center justify-center',
                            on ? 'bg-accent border-accent' : 'border-base-500 hover:border-slate-400',
                          )}
                          aria-label={on ? `Disable ${p.name}` : `Enable ${p.name}`}
                        >
                          {on && <Check className="w-3 h-3 text-base-900" />}
                        </button>

                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2 flex-wrap">
                            <span className="text-sm font-medium">{p.name}</span>
                            {missing && <span className="chip bg-base-600 text-slate-400">not found</span>}
                            {det?.found && det.approxBytes > 0 && (
                              <span className="chip bg-base-600 text-slate-400">~{bytes(det.approxBytes)}</span>
                            )}
                            {p.secrets.length > 0 && (
                              <span className="chip bg-accent/15 text-accent">
                                {p.secrets.length} secret step{p.secrets.length > 1 ? 's' : ''}
                              </span>
                            )}
                            {(p.notes.length > 0 || p.include.length > 0) && (
                              <button
                                className="text-slate-600 hover:text-slate-300"
                                onClick={() => setExpanded(expanded === p.id ? null : p.id)}
                              >
                                <Info className="w-3.5 h-3.5" />
                              </button>
                            )}
                          </div>
                          <p className="text-xs text-slate-500 mt-0.5">{p.description}</p>
                          {det?.detail && (
                            <p className="text-[11px] text-warn/80 mt-1 leading-relaxed">{det.detail}</p>
                          )}

                          {expanded === p.id && (
                            <div className="mt-3 space-y-2 border-t border-base-600 pt-2">
                              {p.notes.map((n, i) => (
                                <p key={i} className="text-[11px] text-slate-400 leading-relaxed">— {n}</p>
                              ))}
                              {p.include.length > 0 && (
                                <div>
                                  <div className="label mb-1">Includes</div>
                                  <div className="space-y-0.5">
                                    {p.include.map((inc) => (
                                      <div key={inc} className="font-mono text-[11px] text-slate-500 selectable">
                                        {inc}
                                      </div>
                                    ))}
                                  </div>
                                </div>
                              )}
                              {det?.paths.length ? (
                                <div>
                                  <div className="label mb-1">Found at</div>
                                  {det.paths.map((path) => (
                                    <div key={path} className="font-mono text-[11px] text-slate-500 selectable">
                                      {path}
                                    </div>
                                  ))}
                                </div>
                              ) : null}
                            </div>
                          )}
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>
          );
        })}

        <section>
          <h2 className="label mb-2 flex items-center gap-1.5">
            <Plus className="w-3.5 h-3.5" /> Custom patterns
          </h2>
          <div className="panel p-3">
            <div className="flex gap-2">
              <input
                className="field flex-1 font-mono text-xs"
                placeholder="%USERPROFILE%/Projects/**/*.psd"
                value={pattern}
                onChange={(e) => setPattern(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    addCustomInclude(pattern);
                    setPattern('');
                  }
                }}
              />
              <button
                className="btn-ghost"
                onClick={() => {
                  addCustomInclude(pattern);
                  setPattern('');
                }}
              >
                Add
              </button>
            </div>
            <p className="text-[11px] text-slate-600 mt-1.5">
              Globs use <code className="text-slate-400">**</code> for any depth and{' '}
              <code className="text-slate-400">*</code> within one segment. <code className="text-slate-400">%ENV%</code>{' '}
              variables are expanded at plan time.
            </p>
            {customIncludes.length > 0 && (
              <div className="mt-3 space-y-1">
                {customIncludes.map((c) => (
                  <div key={c} className="flex items-center gap-2 text-xs">
                    <span className="font-mono text-slate-400 flex-1 selectable">{c}</span>
                    <button className="text-slate-600 hover:text-danger" onClick={() => removeCustomInclude(c)}>
                      <X className="w-3.5 h-3.5" />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        <CredentialPanel />
      </div>
    </div>
  );
}

/** Credential Manager needs a human on the secure desktop — so it gets its own panel. */
function CredentialPanel() {
  const [steps, setSteps] = useState<string[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  return (
    <section>
      <h2 className="label mb-2 flex items-center gap-1.5">
        <KeyRound className="w-3.5 h-3.5" /> Windows Credential Manager
      </h2>
      <div className="panel p-3">
        <p className="text-xs text-slate-400 leading-relaxed">
          Exporting saved Windows credentials to a <code className="text-slate-300">.crd</code> file runs on the
          Ctrl+Alt+Del secure desktop. That's deliberate: no program — including this one — can type the protection
          password for you. Run it manually and save the file into your staging folder.
        </p>
        <div className="mt-2 flex gap-2">
          <button
            className="btn-ghost inline-flex items-center gap-1.5 text-xs"
            onClick={async () => {
              setErr(null);
              try {
                const info = await api.credentialManagerInfo();
                setSteps(info.steps);
                await api.openCredentialWizard();
              } catch (e) {
                setErr((e as Error).message);
              }
            }}
          >
            <ExternalLink className="w-3.5 h-3.5" />
            Open the backup wizard
          </button>
        </div>
        {err && <p className="text-xs text-danger mt-2">{err}</p>}
        {steps && (
          <ol className="mt-3 space-y-1 list-decimal list-inside">
            {steps.map((s, i) => (
              <li key={i} className="text-[11px] text-slate-400 leading-relaxed">{s}</li>
            ))}
          </ol>
        )}
      </div>
    </section>
  );
}

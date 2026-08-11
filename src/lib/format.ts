export function bytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[i]}`;
}

export function count(n: number): string {
  return n.toLocaleString();
}

export function duration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)} s`;
  const m = Math.floor(s / 60);
  return `${m}m ${Math.round(s % 60)}s`;
}

export function when(unixSeconds: number): string {
  if (!unixSeconds) return '—';
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
  });
}

/** Middle-ellipsis so both the drive and the filename stay readable. */
export function shortPath(p: string, max = 64): string {
  if (p.length <= max) return p;
  const head = Math.floor((max - 1) / 2);
  return `${p.slice(0, head)}…${p.slice(p.length - (max - head - 1))}`;
}

export function ext(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

/** Stable colour per extension so the treemap stays readable while drilling. */
export function extColor(name: string, isDir: boolean): string {
  if (isDir) return 'hsl(215 25% 30%)';
  const e = ext(name);
  if (!e) return 'hsl(215 12% 38%)';
  let h = 0;
  for (let i = 0; i < e.length; i++) h = (h * 31 + e.charCodeAt(i)) % 360;
  return `hsl(${h} 45% 42%)`;
}

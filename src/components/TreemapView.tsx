/**
 * Squarified treemap on a canvas.
 *
 * Canvas rather than SVG/recharts: a directory can easily have 2000+ visible
 * children, and 2000 DOM nodes with hover handlers janks the whole window.
 * One canvas plus a hit-test array stays at 60fps and the layout maths is
 * ~60 lines.
 *
 * Layout is Bruls/Huizing/van Wijk squarification — it keeps rectangles close
 * to square so relative areas stay comparable, which is the entire point of a
 * treemap and the thing naive slice-and-dice gets wrong.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { bytes, extColor, shortPath } from '@/lib/format';
import type { TreeNode } from '@/types';

interface Cell {
  node: TreeNode;
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Rect { x: number; y: number; w: number; h: number }

function worst(row: number[], side: number, scale: number): number {
  const sum = row.reduce((a, b) => a + b, 0) * scale;
  const max = Math.max(...row) * scale;
  const min = Math.min(...row) * scale;
  if (sum === 0 || side === 0) return Infinity;
  return Math.max((side * side * max) / (sum * sum), (sum * sum) / (side * side * min));
}

function squarify(nodes: TreeNode[], rect: Rect): Cell[] {
  const cells: Cell[] = [];
  const items = nodes.filter((n) => n.size > 0);
  if (items.length === 0) return cells;

  const total = items.reduce((a, n) => a + n.size, 0);
  const scale = (rect.w * rect.h) / total;

  let { x, y, w, h } = rect;
  let i = 0;

  while (i < items.length) {
    const side = Math.min(w, h);
    const row: TreeNode[] = [];
    const values: number[] = [];

    // Grow the row while it improves the worst aspect ratio.
    while (i < items.length) {
      const next = [...values, items[i].size];
      if (values.length > 0 && worst(next, side, scale) > worst(values, side, scale)) break;
      values.push(items[i].size);
      row.push(items[i]);
      i++;
    }

    const rowArea = values.reduce((a, b) => a + b, 0) * scale;
    const thickness = side === 0 ? 0 : rowArea / side;

    if (w >= h) {
      let cy = y;
      for (const n of row) {
        const cellH = (n.size * scale) / Math.max(thickness, 1e-9);
        cells.push({ node: n, x, y: cy, w: thickness, h: cellH });
        cy += cellH;
      }
      x += thickness;
      w -= thickness;
    } else {
      let cx = x;
      for (const n of row) {
        const cellW = (n.size * scale) / Math.max(thickness, 1e-9);
        cells.push({ node: n, x: cx, y, w: cellW, h: thickness });
        cx += cellW;
      }
      y += thickness;
      h -= thickness;
    }
  }

  return cells;
}

interface Props {
  root: TreeNode | null;
  onDrill: (nodeId: number) => void;
  height?: number;
}

export default function TreemapView({ root, onDrill, height = 420 }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: height });
  const [hover, setHover] = useState<Cell | null>(null);
  const [pointer, setPointer] = useState({ x: 0, y: 0 });

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      setSize({ w: Math.max(200, entry.contentRect.width), h: height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [height]);

  const cells = useMemo(() => {
    if (!root?.children?.length) return [];
    return squarify(root.children, { x: 0, y: 0, w: size.w, h: size.h });
  }, [root, size]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = size.w * dpr;
    canvas.height = size.h * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, size.w, size.h);

    for (const c of cells) {
      const isHover = hover?.node.id === c.node.id;
      ctx.fillStyle = extColor(c.node.name, c.node.isDir);
      ctx.fillRect(c.x, c.y, c.w, c.h);
      if (isHover) {
        ctx.fillStyle = 'rgba(255,255,255,0.14)';
        ctx.fillRect(c.x, c.y, c.w, c.h);
      }
      ctx.strokeStyle = 'rgba(11,13,16,0.85)';
      ctx.lineWidth = 1;
      ctx.strokeRect(c.x + 0.5, c.y + 0.5, c.w - 1, c.h - 1);

      // Only label cells big enough that the text isn't noise.
      if (c.w > 56 && c.h > 22) {
        ctx.fillStyle = 'rgba(255,255,255,0.94)';
        ctx.font = '600 11px ui-sans-serif, system-ui, sans-serif';
        ctx.save();
        ctx.beginPath();
        ctx.rect(c.x + 4, c.y + 3, c.w - 8, c.h - 6);
        ctx.clip();
        ctx.fillText(c.node.name, c.x + 6, c.y + 15);
        if (c.h > 36) {
          ctx.fillStyle = 'rgba(255,255,255,0.62)';
          ctx.font = '10px ui-monospace, monospace';
          ctx.fillText(bytes(c.node.size), c.x + 6, c.y + 28);
        }
        ctx.restore();
      }
    }
  }, [cells, size, hover]);

  const hit = useCallback(
    (mx: number, my: number) => cells.find((c) => mx >= c.x && mx < c.x + c.w && my >= c.y && my < c.y + c.h) ?? null,
    [cells],
  );

  if (!root) {
    return (
      <div className="panel flex items-center justify-center text-sm text-slate-600" style={{ height }}>
        Run a scan to see the treemap.
      </div>
    );
  }

  return (
    <div ref={wrapRef} className="relative panel overflow-hidden" style={{ height }}>
      <canvas
        ref={canvasRef}
        style={{ width: size.w, height: size.h }}
        className="block cursor-pointer"
        onMouseMove={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          const mx = e.clientX - r.left;
          const my = e.clientY - r.top;
          setPointer({ x: mx, y: my });
          setHover(hit(mx, my));
        }}
        onMouseLeave={() => setHover(null)}
        onClick={() => {
          if (hover?.node.isDir && hover.node.id !== 0xffffffff) onDrill(hover.node.id);
        }}
      />

      {hover && (
        <div
          className="pointer-events-none absolute z-10 panel bg-base-900/95 px-3 py-2 text-xs shadow-xl max-w-sm"
          style={{
            left: Math.min(pointer.x + 12, size.w - 300),
            top: Math.min(pointer.y + 12, size.h - 70),
          }}
        >
          <div className="font-semibold">{hover.node.name}</div>
          <div className="text-slate-400 font-mono text-[10px] mt-0.5">
            {shortPath(hover.node.path, 52)}
          </div>
          <div className="mt-1 flex gap-3 text-slate-300">
            <span>{bytes(hover.node.size)}</span>
            {hover.node.isDir && <span>{hover.node.childCount} items</span>}
            {hover.node.isDir && hover.node.id !== 0xffffffff && (
              <span className="text-accent">click to drill in</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/* ==========================================================================
   ReBackUp — page behaviour on top of the motion kit.
   The kit owns preloader / reveal / glow / tilt / magnetic / ripple /
   scroll-progress / back-to-top / cursor / theme. This file owns the things
   that are specific to this page.
   ========================================================================== */
import initMotionKit, { lockScroll, unlockScroll, trapFocus } from '/assets/js/motion-kit.js';
import Lenis from '/assets/js/vendor/lenis.mjs';

const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';

initMotionKit();   // also wires [data-theme-toggle] against the 'mk-theme' key

/* ---- smooth scroll (Lenis) --------------------------------------------
   Native scroll-behavior:smooth only affects programmatic jumps; this is what
   makes the wheel itself feel weighted. Skipped entirely under reduced motion
   — a global CSS rule cannot stop a JS-driven scroll loop.                */
let lenis = null;
if (!REDUCED) {
  lenis = new Lenis({
    duration: 1.15,
    easing: (t) => Math.min(1, 1.001 - Math.pow(2, -10 * t)),
    smoothWheel: true,
    touchMultiplier: 1.6,
  });
  const raf = (time) => { lenis.raf(time); requestAnimationFrame(raf); };
  requestAnimationFrame(raf);
  // CSS smooth-scroll and Lenis fight each other; Lenis wins, so stand CSS down.
  document.documentElement.style.scrollBehavior = 'auto';
}

/* ---- sticky nav ------------------------------------------------------- */
const nav = document.querySelector('[data-nav]');
const onScroll = () => nav && nav.classList.toggle('is-stuck', scrollY > 20);
addEventListener('scroll', onScroll, { passive: true });
onScroll();

/* ---- active section + sliding pill ------------------------------------
   One pill element that physically moves between items rather than
   cross-fading. Section tracking compares each section's top against 35% of
   the viewport — cheaper and far more predictable than IntersectionObserver
   when sections have wildly different heights.                            */
const links = [...document.querySelectorAll('[data-nav-link]')];
const pill = document.querySelector('.nav-pill');
const sections = links
  .map((a) => document.querySelector(a.getAttribute('href')))
  .filter(Boolean);

function movePill(el) {
  if (!pill || !el) return;
  pill.style.width = `${el.offsetWidth}px`;
  pill.style.transform = `translateX(${el.offsetLeft}px)`;
  pill.style.opacity = '1';
}

let activeIdx = -1;
function syncActive() {
  const line = innerHeight * 0.35;
  let idx = -1;
  sections.forEach((s, i) => { if (s.getBoundingClientRect().top <= line) idx = i; });
  if (idx === activeIdx) return;
  activeIdx = idx;
  links.forEach((a, i) => a.classList.toggle('is-active', i === idx));
  if (idx === -1) { if (pill) pill.style.opacity = '0'; } else { movePill(links[idx]); }
}
addEventListener('scroll', syncActive, { passive: true });
addEventListener('resize', () => { activeIdx = -1; syncActive(); });
syncActive();

// Hovering previews the pill; leaving snaps it back to the real active item.
links.forEach((a) => {
  a.addEventListener('mouseenter', () => movePill(a));
  a.addEventListener('focus', () => movePill(a));
});
document.querySelector('.nav-links')?.addEventListener('mouseleave', () => {
  activeIdx === -1 ? (pill && (pill.style.opacity = '0')) : movePill(links[activeIdx]);
});

/* ---- mobile menu: scroll lock + focus trap + Esc ----------------------- */
const menu = document.getElementById('menu');
const openBtn = document.getElementById('menuOpen');
const closeBtn = document.getElementById('menuClose');
let releaseTrap = null;

function openMenu() {
  menu.classList.add('is-open');
  openBtn.setAttribute('aria-expanded', 'true');
  lockScroll();
  lenis?.stop();                 // overflow:hidden alone does not stop Lenis
  releaseTrap = trapFocus(menu);
  closeBtn.focus();
}
function closeMenu() {
  menu.classList.remove('is-open');
  openBtn.setAttribute('aria-expanded', 'false');
  unlockScroll();
  lenis?.start();
  releaseTrap?.();               // the kit's trap restores focus to the trigger
  releaseTrap = null;
}
openBtn?.addEventListener('click', openMenu);
closeBtn?.addEventListener('click', closeMenu);
menu?.querySelectorAll('a').forEach((a) => a.addEventListener('click', closeMenu));
addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && menu?.classList.contains('is-open')) closeMenu();
});

/* ---- count-up stats ---------------------------------------------------
   Driven from performance.now() in a rAF loop rather than setInterval, so it
   stays smooth under load and always lands exactly on the target.         */
const counters = document.querySelectorAll('[data-count]');
const fmt = new Intl.NumberFormat('en-US');

function runCount(el) {
  const target = Number(el.dataset.count) || 0;
  const suffix = el.dataset.suffix || '';
  if (REDUCED) { el.textContent = fmt.format(target) + suffix; return; }
  const dur = 1400;
  const t0 = performance.now();
  const tick = (now) => {
    const p = Math.min(1, (now - t0) / dur);
    const eased = 1 - Math.pow(1 - p, 3);          // easeOutCubic
    el.textContent = fmt.format(Math.round(target * eased)) + (p === 1 ? suffix : '');
    if (p < 1) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

if ('IntersectionObserver' in window) {
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => {
      if (!e.isIntersecting) return;
      runCount(e.target);
      io.unobserve(e.target);
    });
  }, { threshold: 0.6 });
  counters.forEach((c) => io.observe(c));
} else {
  counters.forEach(runCount);
}

/* ---- copy buttons ------------------------------------------------------ */
document.querySelectorAll('[data-copy]').forEach((btn) => {
  btn.addEventListener('click', async () => {
    const code = btn.parentElement?.querySelector('code');
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code.innerText.trim());
      const prev = btn.textContent;
      btn.textContent = 'COPIED';
      setTimeout(() => { btn.textContent = prev; }, 1600);
    } catch {
      btn.textContent = 'COPY FAILED';           // clipboard can be blocked; say so
      setTimeout(() => { btn.textContent = 'COPY'; }, 1600);
    }
  });
});

/* ---- smooth anchor scroll (no dependency) -----------------------------
   One delegated listener. -90px clears the fixed header, and replaceState
   keeps the hash in the URL without a jump or a history entry per click.  */
addEventListener('click', (e) => {
  const a = e.target.closest?.('a[href^="#"]');
  if (!a) return;
  const id = a.getAttribute('href');
  if (!id || id === '#') return;
  const target = document.querySelector(id);
  if (!target) return;
  e.preventDefault();
  if (lenis) {
    lenis.scrollTo(target, { offset: -90, duration: 1.3 });   // -90 clears the fixed header
  } else {
    scrollTo({ top: target.getBoundingClientRect().top + scrollY - 90, behavior: 'auto' });
  }
  history.replaceState(null, '', id);
});

/* Decorative layers must never be announced or clickable. */
document.querySelectorAll('.atmos, .grain, .marquee')
  .forEach((el) => { el.setAttribute('aria-hidden', 'true'); el.style.pointerEvents = 'none'; });

/* ---- interactive carousel ---------------------------------------------
   Scroll-snap does the scrolling; this adds pointer-drag with momentum,
   dots, arrows and keyboard. Because the base is native scrolling, it also
   works mid-download and with JS off — and inherits touch behaviour for
   free instead of reimplementing it badly.                               */
document.querySelectorAll('[data-carousel]').forEach((root) => {
  const view   = root.querySelector('[data-carousel-viewport]');
  const track  = root.querySelector('[data-carousel-track]');
  const dotsEl = root.querySelector('[data-carousel-dots]');
  const prev   = root.querySelector('[data-carousel-prev]');
  const next   = root.querySelector('[data-carousel-next]');
  const slides = [...track.children];
  if (!slides.length) return;

  let index = 0;

  slides.forEach((_, i) => {
    const b = document.createElement('button');
    b.className = 'dot';
    b.type = 'button';
    b.setAttribute('role', 'tab');
    b.setAttribute('aria-label', `Go to slide ${i + 1}`);
    b.addEventListener('click', () => go(i));
    dotsEl.appendChild(b);
  });
  const dots = [...dotsEl.children];

  const centreOf = (el) => el.offsetLeft - (view.clientWidth - el.offsetWidth) / 2;

  function go(i, smooth = true) {
    index = Math.max(0, Math.min(slides.length - 1, i));
    view.scrollTo({ left: centreOf(slides[index]), behavior: smooth && !REDUCED ? 'smooth' : 'auto' });
  }

  // Derive the active slide from actual scroll position rather than tracking
  // it ourselves — drag, wheel, touch and keyboard then all agree.
  function sync() {
    const mid = view.scrollLeft + view.clientWidth / 2;
    let best = 0, bestD = Infinity;
    slides.forEach((s, i) => {
      const d = Math.abs(s.offsetLeft + s.offsetWidth / 2 - mid);
      if (d < bestD) { bestD = d; best = i; }
    });
    index = best;
    slides.forEach((s, i) => s.classList.toggle('is-active', i === best));
    dots.forEach((d, i) => {
      d.classList.toggle('is-active', i === best);
      d.setAttribute('aria-selected', String(i === best));
    });
    if (prev) prev.disabled = view.scrollLeft <= 2;
    if (next) next.disabled = view.scrollLeft >= view.scrollWidth - view.clientWidth - 2;
  }

  let raf = 0;
  view.addEventListener('scroll', () => {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(sync);
  }, { passive: true });

  prev?.addEventListener('click', () => go(index - 1));
  next?.addEventListener('click', () => go(index + 1));

  view.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowRight') { e.preventDefault(); go(index + 1); }
    if (e.key === 'ArrowLeft')  { e.preventDefault(); go(index - 1); }
    if (e.key === 'Home')       { e.preventDefault(); go(0); }
    if (e.key === 'End')        { e.preventDefault(); go(slides.length - 1); }
  });

  /* pointer drag with a little throw. Snap is disabled while dragging so the
     browser doesn't fight the transform, then re-enabled on release.      */
  let down = false, startX = 0, startScroll = 0, lastX = 0, lastT = 0, vel = 0;

  view.addEventListener('pointerdown', (e) => {
    if (e.pointerType === 'touch') return;      // let native touch scrolling win
    down = true; startX = lastX = e.clientX; startScroll = view.scrollLeft;
    lastT = performance.now(); vel = 0;
    view.classList.add('is-dragging');
    view.setPointerCapture(e.pointerId);
  });

  view.addEventListener('pointermove', (e) => {
    if (!down) return;
    const now = performance.now();
    const dt = now - lastT;
    if (dt > 0) vel = (e.clientX - lastX) / dt;
    lastX = e.clientX; lastT = now;
    view.scrollLeft = startScroll - (e.clientX - startX);
  });

  function release(e) {
    if (!down) return;
    down = false;
    view.classList.remove('is-dragging');
    try { view.releasePointerCapture(e.pointerId); } catch {}
    // A flick past the threshold advances a slide; anything slower just snaps
    // to whatever is nearest.
    if (Math.abs(vel) > 0.45) go(index + (vel < 0 ? 1 : -1));
    else go(index);
  }
  view.addEventListener('pointerup', release);
  view.addEventListener('pointercancel', release);
  view.addEventListener('dragstart', (e) => e.preventDefault());

  addEventListener('resize', () => go(index, false));
  sync();
  requestAnimationFrame(() => go(0, false));
});


/* The kit's back-to-top uses window.scrollTo; route it through Lenis so the
   trip back up uses the same easing as everything else. */
document.querySelector('[data-back-to-top]')?.addEventListener('click', (e) => {
  if (!lenis) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  lenis.scrollTo(0, { duration: 1.4 });
}, true);

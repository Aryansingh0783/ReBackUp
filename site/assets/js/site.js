/* ==========================================================================
   ReBackUp — page behaviour on top of the motion kit.
   The kit owns preloader / reveal / glow / tilt / magnetic / ripple /
   scroll-progress / back-to-top / cursor / theme. This file owns the things
   that are specific to this page.
   ========================================================================== */
import initMotionKit, { lockScroll, unlockScroll, trapFocus } from '/assets/js/motion-kit.js';

const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';

initMotionKit();   // also wires [data-theme-toggle] against the 'mk-theme' key

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
  releaseTrap = trapFocus(menu);
  closeBtn.focus();
}
function closeMenu() {
  menu.classList.remove('is-open');
  openBtn.setAttribute('aria-expanded', 'false');
  unlockScroll();
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
  const top = target.getBoundingClientRect().top + scrollY - 90;
  scrollTo({ top, behavior: REDUCED ? 'auto' : 'smooth' });
  history.replaceState(null, '', id);
});

/* Decorative layers must never be announced or clickable. */
document.querySelectorAll('.atmos, .grain, .marquee')
  .forEach((el) => { el.setAttribute('aria-hidden', 'true'); el.style.pointerEvents = 'none'; });

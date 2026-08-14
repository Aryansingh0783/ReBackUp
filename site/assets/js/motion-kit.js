/* ==========================================================================
   motion-kit — vanilla build
   --------------------------------------------------------------------------
   Every JS-driven effect from the React kit, with zero dependencies and no
   build step. Pair with motion-kit.css (which imports tokens + effects).

     <script type="module">
       import { initMotionKit } from './motion-kit.js';
       initMotionKit();
     </script>

   Opt in per element with data attributes:

     <div data-reveal>                     fade + slide + blur on scroll
     <div data-reveal="left" data-delay="120">
     <h1 data-reveal-text>Big headline</h1> word-by-word reveal
     <div data-glow>                       pointer-tracked glow (needs .glow-card)
     <div data-tilt>                       subtle 3D tilt
     <button data-magnetic data-ripple>    magnetic pull + click ripple
     <div data-parallax="0.2">             scroll parallax
     <body data-cursor-ring>               custom cursor + ambient glow

   Everything below no-ops under prefers-reduced-motion.
   ========================================================================== */

const REDUCED = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';

/* -------------------------------------------------------------------------
   1. SCROLL REVEAL  (IntersectionObserver replaces Framer Motion's whileInView)
   ------------------------------------------------------------------------- */
function initReveal() {
  const items = document.querySelectorAll('[data-reveal]');
  if (!items.length) return;

  if (REDUCED) {
    items.forEach((el) => el.classList.add('is-revealed'));
    return;
  }

  items.forEach((el) => {
    const dir = el.dataset.reveal || 'up';
    const map = { up: '0,28px', down: '0,-28px', left: '28px,0', right: '-28px,0', none: '0,0' };
    el.style.setProperty('--mk-from', map[dir] || map.up);
    el.style.transitionDelay = `${el.dataset.delay || 0}ms`;
  });

  const io = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        entry.target.classList.add('is-revealed');
        io.unobserve(entry.target); // reveal once
      });
    },
    // Negative bottom margin ≈ Framer's viewport margin: fires slightly early.
    { rootMargin: '-80px 0px -80px 0px', threshold: 0 }
  );

  items.forEach((el) => io.observe(el));
}

/* -------------------------------------------------------------------------
   2. WORD-BY-WORD HEADLINE
   Wraps each word in an overflow mask. The original text is preserved as
   aria-label so screen readers get one clean sentence.
   ------------------------------------------------------------------------- */
function initRevealText() {
  document.querySelectorAll('[data-reveal-text]').forEach((el) => {
    const text = el.textContent.trim();
    el.setAttribute('aria-label', text);

    el.innerHTML = text
      .split(/\s+/)
      .map(
        (word, i) =>
          `<span class="mk-word-mask"><span class="mk-word" aria-hidden="true" style="transition-delay:${
            i * 55
          }ms">${word}</span></span> `
      )
      .join('');

    if (REDUCED) {
      el.classList.add('is-revealed');
      return;
    }

    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          if (!e.isIntersecting) return;
          e.target.classList.add('is-revealed');
          io.unobserve(e.target);
        });
      },
      { threshold: 0.1 }
    );
    io.observe(el);
  });
}

/* -------------------------------------------------------------------------
   3. POINTER GLOW  — writes CSS custom properties, never touches layout
   ------------------------------------------------------------------------- */
function initGlow() {
  document.querySelectorAll('[data-glow]').forEach((el) => {
    el.addEventListener('mousemove', (e) => {
      const r = el.getBoundingClientRect();
      el.style.setProperty('--mx', `${e.clientX - r.left}px`);
      el.style.setProperty('--my', `${e.clientY - r.top}px`);
    });
  });
}

/* -------------------------------------------------------------------------
   4. TILT — ±5deg max, lerped toward the target each frame for a spring feel
   ------------------------------------------------------------------------- */
function initTilt() {
  if (REDUCED || window.matchMedia('(hover: none)').matches) return;

  document.querySelectorAll('[data-tilt]').forEach((el) => {
    const max = parseFloat(el.dataset.tilt) || 5;
    let tx = 0, ty = 0, cx = 0, cy = 0, raf = 0, running = false;

    const loop = () => {
      cx += (tx - cx) * 0.12;
      cy += (ty - cy) * 0.12;
      el.style.transform = `perspective(1200px) rotateX(${cy}deg) rotateY(${cx}deg)`;
      if (Math.abs(tx - cx) > 0.01 || Math.abs(ty - cy) > 0.01) {
        raf = requestAnimationFrame(loop);
      } else {
        running = false;
      }
    };

    const start = () => {
      if (!running) { running = true; raf = requestAnimationFrame(loop); }
    };

    el.addEventListener('mousemove', (e) => {
      const r = el.getBoundingClientRect();
      tx = ((e.clientX - r.left) / r.width - 0.5) * 2 * max;
      ty = -((e.clientY - r.top) / r.height - 0.5) * 2 * max;
      start();
    });

    el.addEventListener('mouseleave', () => { tx = 0; ty = 0; start(); });
    el.addEventListener('mkdestroy', () => cancelAnimationFrame(raf));
  });
}

/* -------------------------------------------------------------------------
   5. MAGNETIC BUTTONS + RIPPLE
   ------------------------------------------------------------------------- */
function initMagnetic() {
  if (!REDUCED) {
    document.querySelectorAll('[data-magnetic]').forEach((el) => {
      const strength = parseFloat(el.dataset.magnetic) || 0.35;
      el.style.transition = `transform 400ms ${EASE}`;

      el.addEventListener('mousemove', (e) => {
        const r = el.getBoundingClientRect();
        const dx = (e.clientX - (r.left + r.width / 2)) * strength;
        const dy = (e.clientY - (r.top + r.height / 2)) * strength;
        el.style.transition = 'transform 120ms linear'; // responsive while tracking
        el.style.transform = `translate(${dx}px, ${dy}px)`;
      });

      el.addEventListener('mouseleave', () => {
        el.style.transition = `transform 500ms ${EASE}`; // slow settle back
        el.style.transform = 'translate(0,0)';
      });
    });
  }

  document.querySelectorAll('[data-ripple]').forEach((el) => {
    if (getComputedStyle(el).position === 'static') el.style.position = 'relative';
    el.style.overflow = 'hidden';

    el.addEventListener('click', (e) => {
      const r = el.getBoundingClientRect();
      const span = document.createElement('span');
      span.className = 'ripple';
      span.style.left = `${e.clientX - r.left}px`;
      span.style.top = `${e.clientY - r.top}px`;
      el.appendChild(span);
      setTimeout(() => span.remove(), 620);
    });
  });
}

/* -------------------------------------------------------------------------
   6. PARALLAX + SCROLL PROGRESS + BACK TO TOP + ACTIVE SECTION
   One rAF-throttled scroll listener drives all four.
   ------------------------------------------------------------------------- */
function initScrollEffects() {
  const parallax = [...document.querySelectorAll('[data-parallax]')];
  const bar = document.querySelector('[data-scroll-progress]');
  const toTop = document.querySelector('[data-back-to-top]');
  const navLinks = [...document.querySelectorAll('[data-nav-link]')];
  const sections = navLinks
    .map((a) => document.querySelector(a.getAttribute('href')))
    .filter(Boolean);

  let ticking = false;

  const update = () => {
    ticking = false;
    const y = window.scrollY;

    if (bar) {
      const max = document.documentElement.scrollHeight - window.innerHeight;
      bar.style.transform = `scaleX(${max > 0 ? y / max : 0})`;
    }

    if (!REDUCED) {
      parallax.forEach((el) => {
        const speed = parseFloat(el.dataset.parallax) || 0.18;
        const r = el.getBoundingClientRect();
        if (r.bottom < 0 || r.top > window.innerHeight) return;
        el.style.transform = `translate3d(0, ${-r.top * speed}px, 0)`;
      });
    }

    if (toTop) toTop.classList.toggle('is-visible', y > window.innerHeight * 0.9);

    document.querySelector('[data-nav]')?.classList.toggle('is-scrolled', y > 20);

    if (sections.length) {
      const line = window.innerHeight * 0.35;
      let activeId = sections[0].id;
      sections.forEach((s) => {
        if (s.getBoundingClientRect().top - line <= 0) activeId = s.id;
      });
      navLinks.forEach((a) =>
        a.classList.toggle('is-active', a.getAttribute('href') === `#${activeId}`)
      );
    }
  };

  const onScroll = () => {
    if (!ticking) { ticking = true; requestAnimationFrame(update); }
  };

  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('resize', onScroll);
  update();

  toTop?.addEventListener('click', () =>
    window.scrollTo({ top: 0, behavior: REDUCED ? 'auto' : 'smooth' })
  );
}

/* -------------------------------------------------------------------------
   7. HOVER VIDEO
   ------------------------------------------------------------------------- */
function initHoverVideo() {
  if (REDUCED) return;

  document.querySelectorAll('[data-hover-video]').forEach((card) => {
    const video = card.querySelector('video');
    if (!video) return;

    const play = () => {
      video.currentTime = 0;
      video.play().then(() => card.classList.add('is-playing')).catch(() => {});
    };
    const stop = () => {
      card.classList.remove('is-playing');
      video.pause();
      video.currentTime = 0;
    };

    video.addEventListener('error', () => { stop(); video.remove(); });

    card.addEventListener('mouseenter', play);
    card.addEventListener('mouseleave', stop);
    card.addEventListener('focusin', play);   // keyboard users too
    card.addEventListener('focusout', stop);
  });
}

/* -------------------------------------------------------------------------
   8. CUSTOM CURSOR + AMBIENT MOUSE GLOW  (desktop only)
   ------------------------------------------------------------------------- */
function initCursor() {
  if (REDUCED || !window.matchMedia('(min-width: 1024px) and (hover: hover)').matches) return;

  const dot = document.createElement('span');
  const ring = document.createElement('span');
  const glow = document.createElement('span');
  dot.className = 'mk-cursor-dot';
  ring.className = 'mk-cursor-ring';
  glow.className = 'mk-mouse-glow';
  [dot, ring, glow].forEach((el) => {
    el.setAttribute('aria-hidden', 'true');
    document.body.appendChild(el);
  });

  let mx = 0, my = 0, rx = 0, ry = 0, gx = 0, gy = 0;

  window.addEventListener('mousemove', (e) => {
    mx = e.clientX;
    my = e.clientY;
    dot.style.transform = `translate3d(${mx}px, ${my}px, 0)`;
    const interactive = e.target.closest('a, button, [data-cursor="hover"]');
    ring.classList.toggle('is-active', Boolean(interactive));
    dot.classList.toggle('is-active', Boolean(interactive));
  }, { passive: true });

  // Two different lerp rates: the ring follows closely, the glow drifts far behind.
  const loop = () => {
    rx += (mx - rx) * 0.18;
    ry += (my - ry) * 0.18;
    gx += (mx - gx) * 0.045;
    gy += (my - gy) * 0.045;
    ring.style.transform = `translate3d(${rx}px, ${ry}px, 0)`;
    glow.style.transform = `translate3d(${gx}px, ${gy}px, 0)`;
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

/* -------------------------------------------------------------------------
   9. SCROLL LOCK  (for modals / menus) — reference counted
   ------------------------------------------------------------------------- */
let locks = 0;
let savedPadding = '';

export function lockScroll() {
  locks += 1;
  if (locks > 1) return;
  const bar = window.innerWidth - document.documentElement.clientWidth;
  savedPadding = document.body.style.paddingRight;
  if (bar > 0) document.body.style.paddingRight = `${bar}px`;
  document.body.style.overflow = 'hidden';
}

export function unlockScroll() {
  locks = Math.max(0, locks - 1);
  if (locks > 0) return;
  document.body.style.overflow = '';
  document.body.style.paddingRight = savedPadding;
}

/* -------------------------------------------------------------------------
   10. FOCUS TRAP  (for modals)
   ------------------------------------------------------------------------- */
const FOCUSABLE =
  'a[href], button:not([disabled]), textarea, input, select, [tabindex]:not([tabindex="-1"])';

export function trapFocus(container) {
  const previous = document.activeElement;
  const items = () => [...container.querySelectorAll(FOCUSABLE)].filter((el) => el.offsetParent);

  setTimeout(() => (items()[0] || container).focus({ preventScroll: true }), 60);

  const onKey = (e) => {
    if (e.key !== 'Tab') return;
    const list = items();
    if (!list.length) return;
    const first = list[0];
    const last = list[list.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  };

  container.addEventListener('keydown', onKey);

  return () => {
    container.removeEventListener('keydown', onKey);
    previous?.focus?.({ preventScroll: true });
  };
}

/* -------------------------------------------------------------------------
   11. THEME  — call before first paint (inline in <head>) to avoid a flash
   ------------------------------------------------------------------------- */
export function initTheme(storageKey = 'mk-theme') {
  const stored = localStorage.getItem(storageKey);
  const dark = stored ? stored === 'dark' : window.matchMedia('(prefers-color-scheme: dark)').matches;
  document.documentElement.classList.toggle('dark', dark);

  document.querySelectorAll('[data-theme-toggle]').forEach((btn) =>
    btn.addEventListener('click', () => {
      const next = !document.documentElement.classList.contains('dark');
      document.documentElement.classList.toggle('dark', next);
      localStorage.setItem(storageKey, next ? 'dark' : 'light');
    })
  );
}

/* -------------------------------------------------------------------------
   12. PRELOADER
   ------------------------------------------------------------------------- */
function initPreloader() {
  const el = document.querySelector('[data-preloader]');
  if (!el) return;

  if (REDUCED) { el.remove(); return; }

  const counter = el.querySelector('[data-preloader-count]');
  const barEl = el.querySelector('[data-preloader-bar]');
  const total = parseInt(el.dataset.preloader, 10) || 1250;
  const start = performance.now();
  lockScroll();

  const tick = (now) => {
    const p = Math.min(1, (now - start) / total);
    if (counter) counter.textContent = String(Math.round(p * 100)).padStart(3, '0');
    if (barEl) barEl.style.transform = `scaleX(${p})`;
    if (p < 1) requestAnimationFrame(tick);
    else {
      el.classList.add('is-done');
      unlockScroll();
      setTimeout(() => el.remove(), 1000);
    }
  };
  requestAnimationFrame(tick);
}

/* ------------------------------------------------------------------------- */
export function initMotionKit() {
  initPreloader();
  initReveal();
  initRevealText();
  initGlow();
  initTilt();
  initMagnetic();
  initScrollEffects();
  initHoverVideo();
  initCursor();
  initTheme();
}

export default initMotionKit;

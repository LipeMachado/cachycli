"use client";

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import Shot from "@/components/ui/shot";
import { PROVIDERS } from "@/lib/content";

const EASE = [0.22, 1, 0.36, 1] as const;

/** The tab strip + panel, shared by the pinned and the plain layouts. */
function BackendPanel({
  active,
  onSelect,
  pinned = false,
}: {
  active: number;
  onSelect: (index: number) => void;
  pinned?: boolean;
}) {
  const backend = PROVIDERS[active];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        className="flex flex-wrap items-center gap-2 border-b pb-3"
        style={{ borderColor: "rgba(25,26,31,0.16)" }}
        role="tablist"
        aria-label="Providers"
      >
        {PROVIDERS.map((b, i) => {
          const selected = i === active;
          return (
            <button
              key={b.id}
              type="button"
              role="tab"
              aria-selected={selected}
              onClick={() => onSelect(i)}
              className="flex items-baseline gap-2 px-3.5 py-2.5 transition-colors duration-300 sm:px-4 sm:py-3"
              style={{
                background: selected ? "#191a1f" : "transparent",
                color: selected ? "#e7e4dd" : "rgba(25,26,31,0.72)",
              }}
            >
              <span className="mono-sm tnum" style={{ color: selected ? "#e0af68" : "inherit" }}>
                {b.index}
              </span>
              <span className="display text-[1rem] sm:text-[1.2rem] md:text-[1.35rem]">{b.name}</span>
            </button>
          );
        })}

        <div className="ml-auto hidden items-center gap-3 lg:flex">
          <span className="mono-sm tnum" style={{ color: "rgba(25,26,31,0.68)" }}>
            {PROVIDERS[active].index} / {String(PROVIDERS.length).padStart(2, "0")}
          </span>
          <span className="relative block h-[2px] w-24 overflow-hidden" style={{ background: "rgba(25,26,31,0.18)" }}>
            <motion.span
              className="absolute inset-y-0 left-0"
              style={{ background: "#191a1f" }}
              animate={{ width: `${((active + 1) / PROVIDERS.length) * 100}%` }}
              transition={{ duration: 0.4, ease: EASE }}
            />
          </span>
          <span className="mono-sm" style={{ color: "rgba(25,26,31,0.68)" }}>
            {pinned ? "keep scrolling" : "select"}
          </span>
        </div>
      </div>

      <div className="mt-5 grid min-h-0 flex-1 gap-8 lg:mt-6 lg:grid-cols-[1fr_1.08fr] lg:gap-12">
        <AnimatePresence mode="wait">
          <motion.div
            key={backend.id}
            initial={{ opacity: 0, y: 14, filter: "blur(8px)" }}
            animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
            exit={{ opacity: 0, y: -10, filter: "blur(8px)" }}
            transition={{ duration: 0.4, ease: EASE }}
            className="min-h-0 overflow-y-auto pr-1"
          >
            <p className="display h3">{backend.tagline}</p>
            <p className="muted mt-3 text-[0.98rem] leading-relaxed">{backend.body}</p>

            <ul className="mt-4 space-y-2">
              {backend.points.map((point) => (
                <li key={point} className="flex items-baseline gap-3 text-[0.92rem]">
                  <span style={{ color: "#b07d24" }}>▪</span>
                  <span>{point}</span>
                </li>
              ))}
            </ul>

            <pre className="codeblock mt-5 hidden text-[0.78rem] sm:block">{backend.config}</pre>

            <p className="code mt-4 hidden sm:block" style={{ color: "rgba(25,26,31,0.72)" }}>
              $ {backend.command}
            </p>
          </motion.div>
        </AnimatePresence>

        <AnimatePresence mode="wait">
          <motion.figure
            key={backend.id}
            initial={{ opacity: 0, y: 16, filter: "blur(10px)" }}
            animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
            exit={{ opacity: 0, y: -12, filter: "blur(10px)" }}
            transition={{ duration: 0.45, ease: EASE }}
            className="flex min-h-0 items-start"
          >
            <div
              className="shell max-h-full w-full overflow-hidden"
              style={{ borderColor: "rgba(25,26,31,0.35)" }}
            >
              <div className="shell-bar shrink-0">
                <div className="shell-dots" aria-hidden="true">
                  <i />
                  <i />
                  <i />
                </div>
                <span className="truncate">{backend.shotCaption}</span>
                <span className="ml-auto shrink-0" style={{ color: "var(--accent)" }}>
                  {backend.name}
                </span>
              </div>
              <span className="relative block">
                <Shot
                  src={backend.shot}
                  alt={`${backend.name} running in CachyCLI`}
                  width={backend.shotW}
                  height={backend.shotH}
                  sizes="(max-width: 1024px) 100vw, 620px"
                  quality={90}
                  mode="intrinsic"
                  className="max-h-full w-full object-contain object-top"
                />
              </span>
            </div>
          </motion.figure>
        </AnimatePresence>
      </div>
    </div>
  );
}

/**
 * Providers: on wide screens the whole section pins for one screen of scrolling
 * scrolling and the selection advances 01 → 02 → 03 before the page continues.
 */
export default function BackendSwitcher() {
  const reduceMotion = useReducedMotion();
  const [isWide, setIsWide] = useState(false);
  const [active, setActive] = useState(0);
  const wrap = useRef<HTMLDivElement>(null);
  // Refs so the scroll handler reads the live mode instead of a stale closure.
  const panelMode = useRef<"plain" | "pin">("plain");

  useEffect(() => {
    const query = window.matchMedia("(min-width: 1100px) and (min-height: 700px)");
    const update = () => setIsWide(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    panelMode.current = isWide && !reduceMotion ? "pin" : "plain";
  }, [isWide, reduceMotion]);

  // Scroll-driven index: native listener + rAF (works with smooth scrolling).
  useEffect(() => {
    if (!isWide || reduceMotion) return;
    const el = wrap.current;
    if (!el) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const rect = el.getBoundingClientRect();
      const total = rect.height - window.innerHeight;
      const scrolled = Math.min(Math.max(-rect.top, 0), Math.max(total, 1));
      const progress = total > 0 ? scrolled / total : 0;
      const index = Math.min(
        PROVIDERS.length - 1,
        Math.floor(progress * PROVIDERS.length * 0.999),
      );
      setActive((previous) => (previous === index ? previous : index));
    };
    const onScroll = () => {
      if (!frame) frame = window.requestAnimationFrame(update);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    update();
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [isWide, reduceMotion]);

  // Plain layout: small screens or reduced motion.
  if (!isWide || reduceMotion) {
    return (
      <div>
        <BackendPanel active={active} onSelect={setActive} />
      </div>
    );
  }

  return (
    <div ref={wrap} className="pin-wrap" style={{ height: `${PROVIDERS.length * 100}vh` }}>
      <div className="pin-stage flex items-start">
        <div className="h-full w-full">
          <BackendPanel
            active={active}
            pinned
            onSelect={() => {
              /* pinned mode follows the scroll position */
            }}
          />
        </div>
      </div>
    </div>
  );
}

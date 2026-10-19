"use client";

import { useCallback, useEffect, useRef, useState } from "react";

export type RailItem = { id: string; index: string; label: string };

type Tone = "dark" | "paper";

/**
 * Fixed right-edge rail: one tick per section, with the active one marked.
 * Colours follow whatever is behind the rail — light marks on dark sections,
 * dark marks on the light (paper) section — and clicking scrolls without
 * touching the URL.
 */
export default function TickRail({ items }: { items: RailItem[] }) {
  const [active, setActive] = useState(items[0]?.id ?? "");
  const [tone, setTone] = useState<Tone>("dark");
  const frame = useRef(0);

  const measure = useCallback(() => {
    frame.current = 0;
    if (typeof window === "undefined") return;

    // Which section owns the middle of the viewport?
    const middle = window.innerHeight / 2;
    let current = "";
    for (const item of items) {
      const el = document.getElementById(item.id);
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (rect.top <= middle && rect.bottom > middle) {
        current = item.id;
        break;
      }
    }
    if (current) setActive((prev) => (prev === current ? prev : current));

    // What colour is behind the rail? (probe left of the rail's hit area)
    const probe = document.elementFromPoint(window.innerWidth - 140, middle);
    const toneHost = probe?.closest("[data-tone]");
    const nextTone: Tone = toneHost?.getAttribute("data-tone") === "paper" ? "paper" : "dark";
    setTone((prev) => (prev === nextTone ? prev : nextTone));
  }, [items]);

  useEffect(() => {
    const onScroll = () => {
      if (!frame.current) frame.current = window.requestAnimationFrame(measure);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    measure();
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (frame.current) window.cancelAnimationFrame(frame.current);
    };
  }, [measure]);

  const on = tone === "paper";

  return (
    <nav
      aria-label="Sections"
      className="fixed right-4 top-1/2 z-30 hidden -translate-y-1/2 flex-col items-end gap-1 xl:flex"
    >
      {items.map((item) => {
        const isActive = active === item.id;
        const markColor = isActive ? "var(--accent)" : on ? "rgba(25,26,31,0.38)" : "var(--line-strong)";
        const numberColor = isActive
          ? on
            ? "#6f5219"
            : "var(--accent)"
          : on
            ? "rgba(25,26,31,0.66)"
            : "var(--ink-38)";
        return (
          <button
            key={item.id}
            type="button"
            title={item.label}
            aria-label={item.label}
            aria-current={isActive ? "true" : undefined}
            onClick={() => {
              document.getElementById(item.id)?.scrollIntoView({ behavior: "smooth", block: "start" });
            }}
            className="group flex items-center gap-2 py-1.5 pl-3"
          >
            <span
              className="mono-sm tnum transition-all duration-300"
              style={{ color: numberColor, opacity: isActive ? 1 : 0 }}
            >
              {item.index}
            </span>
            <span
              className="block h-[1px] transition-all duration-500"
              style={{
                width: isActive ? 28 : 12,
                background: markColor,
              }}
            />
          </button>
        );
      })}
    </nav>
  );
}

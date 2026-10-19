"use client";

import { useEffect } from "react";
import Lenis from "lenis";
import { setLenis } from "@/lib/scroll";

/** Smooth scrolling with Lenis; disabled for reduced-motion users. */
export default function SmoothScroll() {
  useEffect(() => {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const coarse = window.matchMedia("(pointer: coarse)").matches;
    if (reduced || coarse) return; // native scrolling is smoother on touch
    // Small lerp = stops right where you stopped (no drifting past the target).
    const lenis = new Lenis({
      lerp: 0.22,
      wheelMultiplier: 1,
      touchMultiplier: 1.4,
      smoothWheel: true,
      autoRaf: false,
    });
    setLenis(lenis);
    let frame = 0;
    const raf = (time: number) => {
      lenis.raf(time);
      frame = requestAnimationFrame(raf);
    };
    frame = requestAnimationFrame(raf);
    return () => {
      cancelAnimationFrame(frame);
      lenis.destroy();
      setLenis(null);
    };
  }, []);
  return null;
}

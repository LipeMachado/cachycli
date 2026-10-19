"use client";

import { motion, useScroll, useSpring, useTransform } from "motion/react";
import type { ReactNode } from "react";

const EASE = [0.22, 1, 0.36, 1] as const;

/**
 * Blur reveal: children fade, rise and unblur as they enter the viewport and
 * blur back out when they leave (unless `once`). Pass blur={0} for big media —
 * filter animations are the most expensive part, so images only move and fade.
 */
export function Reveal({
  children,
  delay = 0,
  y = 20,
  blur = 6,
  amount = 0.25,
  once = false,
  className = "",
}: {
  children: ReactNode;
  delay?: number;
  y?: number;
  blur?: number;
  amount?: number;
  once?: boolean;
  className?: string;
}) {
  const useBlur = blur > 0;
  return (
    <motion.div
      className={className}
      initial={{ opacity: 0, y, ...(useBlur ? { filter: `blur(${blur}px)` } : {}) }}
      whileInView={{ opacity: 1, y: 0, ...(useBlur ? { filter: "blur(0px)" } : {}) }}
      viewport={{ once, amount, margin: "0px 0px -8% 0px" }}
      transition={{ duration: 0.75, delay, ease: EASE }}
    >
      {children}
    </motion.div>
  );
}

/** Staggered blur reveal for lists. */
export function Stagger({
  children,
  className = "",
  step = 0.05,
  blur = 5,
}: {
  children: ReactNode[];
  className?: string;
  step?: number;
  blur?: number;
}) {
  const useBlur = blur > 0;
  return (
    <div className={className}>
      {children.map((child, i) => (
        <motion.div
          key={i}
          initial={{ opacity: 0, y: 14, ...(useBlur ? { filter: `blur(${blur}px)` } : {}) }}
          whileInView={{ opacity: 1, y: 0, ...(useBlur ? { filter: "blur(0px)" } : {}) }}
          viewport={{ once: false, amount: 0.2, margin: "0px 0px -6% 0px" }}
          transition={{ duration: 0.65, delay: i * step, ease: EASE }}
        >
          {child}
        </motion.div>
      ))}
    </div>
  );
}

/** Thin scroll progress bar. */
export function ScrollProgress() {
  const { scrollYProgress } = useScroll();
  const scaleX = useSpring(scrollYProgress, { stiffness: 130, damping: 30, mass: 0.25 });
  return (
    <motion.div
      aria-hidden
      className="fixed left-0 right-0 top-0 z-50 h-[2px] origin-left"
      style={{ scaleX, background: "var(--accent)" }}
    />
  );
}

/** Gentle parallax for hero media (transform only). */
export function Parallax({
  children,
  distance = 32,
  className = "",
}: {
  children: ReactNode;
  distance?: number;
  className?: string;
}) {
  const { scrollY } = useScroll();
  const y = useTransform(scrollY, [0, 900], [0, distance]);
  return (
    <motion.div className={className} style={{ y }}>
      {children}
    </motion.div>
  );
}

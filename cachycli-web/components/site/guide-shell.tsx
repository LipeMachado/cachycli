"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import Popover from "@/components/ui/popover";
import { Container } from "@/components/ui/section";
import { GUIDE_TOPICS, neighbours, topicBySlug, topicHref } from "@/lib/guide";

const EASE = [0.22, 1, 0.36, 1] as const;

/** Guide shell: topic sidebar, mobile topic picker and prev/next pager. */
export default function GuideShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const slug = pathname.replace(/^\/guide\/?/, "").replace(/\/$/, "");
  const current = topicBySlug(slug) ?? topicBySlug("");
  const { previous, next } = neighbours(slug);
  const [open, setOpen] = useState(false);

  return (
    <div style={{ paddingTop: "calc(var(--nav-h) + clamp(28px, 4vw, 56px))" }}>
      <Container>
        <div className="rule-t flex flex-wrap items-center gap-x-5 gap-y-3 pt-4">
          <span className="mono-sm" style={{ color: "var(--ink-38)" }}>
            Guide
          </span>
          <span className="mono-sm" style={{ color: "var(--accent)" }}>
            {current?.index} / {current?.title}
          </span>

          <div className="ml-auto lg:hidden">
            <Popover
              label="Choose a topic"
              align="right"
              width={300}
              trigger={
                <span className="btn btn-ghost btn-sm">
                  Topics
                  <span aria-hidden>▾</span>
                </span>
              }
            >
              {(close) => (
                <div className="max-h-[60vh] overflow-y-auto py-1">
                  {GUIDE_TOPICS.map((topic) => {
                    const active = topic.slug === slug;
                    return (
                      <Link
                        key={topic.slug || "overview"}
                        href={topicHref(topic)}
                        onClick={close}
                        className="popover-item mono-sm"
                        style={{ color: active ? "var(--accent)" : "var(--ink-60)" }}
                      >
                        <span className="tnum">{topic.index}</span>
                        <span>{topic.nav ?? topic.title}</span>
                      </Link>
                    );
                  })}
                </div>
              )}
            </Popover>
          </div>
        </div>

        <div className="mt-10 grid gap-10 lg:grid-cols-[236px_1fr] lg:gap-14">
          <aside className="sticky top-[calc(var(--nav-h)+24px)] hidden max-h-[calc(100vh-120px)] flex-col overflow-y-auto pr-2 lg:flex">
            <nav aria-label="Guide topics" className="space-y-0.5">
              {GUIDE_TOPICS.map((topic) => {
                const active = topic.slug === slug;
                return (
                  <Link
                    key={topic.slug || "overview"}
                    href={topicHref(topic)}
                    className="group flex items-baseline gap-3 py-2 transition-colors duration-300"
                    style={{ color: active ? "var(--ink)" : "var(--ink-38)" }}
                  >
                    <span
                      className="mono-sm tnum"
                      style={{ color: active ? "var(--accent)" : "inherit" }}
                    >
                      {topic.index}
                    </span>
                    <span className="text-[0.92rem] transition-transform duration-300 group-hover:translate-x-0.5">
                      {topic.nav ?? topic.title}
                    </span>
                  </Link>
                );
              })}
            </nav>
          </aside>

          <div className="min-w-0">
            <AnimatePresence mode="wait">
              <motion.div
                key={pathname}
                initial={{ opacity: 0, y: 12, filter: "blur(8px)" }}
                animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
                exit={{ opacity: 0, y: -8, filter: "blur(8px)" }}
                transition={{ duration: 0.35, ease: EASE }}
              >
                {children}
              </motion.div>
            </AnimatePresence>

            <nav
              className="mt-14 grid gap-4 border-t pt-6 sm:grid-cols-2"
              style={{ borderColor: "var(--line)", marginBottom: "clamp(64px, 8vw, 128px)" }}
              aria-label="Guide pagination"
            >
              {previous ? (
                <Link
                  href={topicHref(previous)}
                  className="panel panel-hover panel-pad group flex flex-col"
                >
                  <span className="mono-sm" style={{ color: "var(--ink-38)" }}>
                    ← Previous
                  </span>
                  <span className="display mt-2 text-[1.15rem]">{previous.title}</span>
                </Link>
              ) : (
                <span />
              )}
              {next ? (
                <Link
                  href={topicHref(next)}
                  className="panel panel-hover panel-pad group flex flex-col sm:items-end sm:text-right"
                >
                  <span className="mono-sm" style={{ color: "var(--ink-38)" }}>
                    Next →
                  </span>
                  <span className="display mt-2 text-[1.15rem]">{next.title}</span>
                </Link>
              ) : null}
            </nav>
          </div>
        </div>
      </Container>
    </div>
  );
}

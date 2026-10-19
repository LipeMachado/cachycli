"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import Popover from "@/components/ui/popover";
import { ButtonLink } from "@/components/ui/button";
import { GITHUB, RAW_INSTALL, RAW_INSTALL_PS1 } from "@/lib/content";
import { NAV_LINKS, activeNavHref } from "@/lib/guide";
import { lockScroll } from "@/lib/scroll";

const EASE = [0.22, 1, 0.36, 1] as const;

export function Logo({ label = true }: { label?: boolean }) {
  return (
    <span className="flex items-center gap-2.5">
      <svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">
        <rect x="0.8" y="0.8" width="22.4" height="22.4" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <path d="M6.6 6.8 11.2 12 6.6 17.2" fill="none" stroke="currentColor" strokeWidth="1.7" />
        <path d="M13 16.8h4.6" stroke="var(--accent)" strokeWidth="1.7" />
      </svg>
      {label ? (
        <span className="mono" style={{ letterSpacing: "0.18em", fontSize: "0.76rem" }}>
          CachyCLI
        </span>
      ) : null}
    </span>
  );
}

export default function Nav() {
  const pathname = usePathname();
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 10);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => setOpen(false), [pathname]);

  useEffect(() => {
    lockScroll(open);
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      lockScroll(false);
    };
  }, [open]);

  return (
    <>
      <header
        className="fixed inset-x-0 top-0 z-40"
        style={{
          background: scrolled ? "rgba(20, 21, 25, 0.72)" : "var(--bg)",
          backdropFilter: scrolled ? "blur(18px) saturate(140%)" : "none",
          WebkitBackdropFilter: scrolled ? "blur(18px) saturate(140%)" : "none",
          borderBottom: `1px solid ${scrolled ? "var(--line)" : "transparent"}`,
          transition: "background-color .45s var(--ease), border-color .45s var(--ease)",
        }}
      >
        <div
          className="mx-auto flex h-[var(--nav-h)] items-center gap-4 px-[var(--pad)]"
          style={{ maxWidth: "var(--maxw)" }}
        >
          <Link href="/" aria-label="CachyCLI home" className="shrink-0" style={{ color: "var(--ink)" }}>
            <Logo />
          </Link>

          <nav className="ml-auto hidden items-center gap-1 lg:flex" aria-label="Main">
            {NAV_LINKS.map((link) => {
              const active = activeNavHref(pathname) === link.href;
              return (
                <Link
                  key={link.label}
                  href={link.href}
                  className="mono-sm relative px-3 py-2 transition-colors duration-300"
                  style={{ color: active ? "var(--ink)" : "var(--ink-60)" }}
                >
                  {link.label}
                  {active ? (
                    <motion.span
                      layoutId="nav-active"
                      className="absolute inset-x-2 -bottom-0.5 h-[1px]"
                      style={{ background: "var(--accent)" }}
                      transition={{ duration: 0.4, ease: EASE }}
                    />
                  ) : null}
                </Link>
              );
            })}
          </nav>

          <div className="ml-auto flex items-center gap-2 lg:ml-4">
            <Popover
              label="Install CachyCLI"
              align="right"
              width={360}
              trigger={
                <span className="btn btn-primary btn-sm hidden sm:inline-flex">Install</span>
              }
            >
              {(close) => (
                <div>
                  <p className="mono-sm border-b px-4 py-3" style={{ borderColor: "var(--line)", color: "var(--ink-38)" }}>
                    Copy and run
                  </p>
                  <div className="space-y-3 p-4">
                    <CommandLine label="Linux · macOS" value={`curl -fsSL ${RAW_INSTALL} | bash`} />
                    <CommandLine label="Windows" value={`irm ${RAW_INSTALL_PS1} | iex`} />
                  </div>
                  <Link
                    href="/guide/installation"
                    onClick={close}
                    className="mono-sm flex items-center gap-2 border-t px-4 py-3 transition-colors duration-300 hover:text-[var(--accent)]"
                    style={{ borderColor: "var(--line)", color: "var(--ink-60)" }}
                  >
                    Full installation guide →
                  </Link>
                </div>
              )}
            </Popover>

            <button
              type="button"
              aria-label={open ? "Close menu" : "Open menu"}
              aria-expanded={open}
              onClick={() => setOpen((v) => !v)}
              className="relative flex h-11 w-11 items-center justify-center lg:hidden"
              style={{ border: "1px solid var(--line-strong)" }}
            >
              <motion.span
                className="absolute block h-[1px] w-4 bg-current"
                animate={open ? { rotate: 45, y: 0 } : { rotate: 0, y: -3.5 }}
                transition={{ duration: 0.32, ease: EASE }}
              />
              <motion.span
                className="absolute block h-[1px] w-4 bg-current"
                animate={open ? { rotate: -45, y: 0 } : { rotate: 0, y: 3.5 }}
                transition={{ duration: 0.32, ease: EASE }}
              />
            </button>
          </div>
        </div>
      </header>

      <AnimatePresence>
        {open && (
          <motion.div
            className="fixed inset-0 z-30 lg:hidden"
            style={{ background: "var(--bg)", paddingTop: "var(--nav-h)" }}
            initial={{ opacity: 0, filter: "blur(12px)" }}
            animate={{ opacity: 1, filter: "blur(0px)" }}
            exit={{ opacity: 0, filter: "blur(12px)" }}
            transition={{ duration: 0.3, ease: EASE }}
          >
            <nav
              className="flex h-full flex-col overflow-y-auto px-[var(--pad)] pb-10 pt-6"
              aria-label="Mobile"
            >
              {NAV_LINKS.map((link, i) => (
                <motion.div
                  key={link.label}
                  initial={{ opacity: 0, y: 18, filter: "blur(6px)" }}
                  animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
                  transition={{ delay: 0.05 + i * 0.045, duration: 0.45, ease: EASE }}
                >
                  <Link
                    href={link.href}
                    className="display flex items-baseline justify-between border-b py-4 text-[2rem]"
                    style={{ borderColor: "var(--line)" }}
                  >
                    {link.label}
                    <span className="mono-sm" style={{ color: "var(--accent)" }}>
                      {String(i + 1).padStart(2, "0")}
                    </span>
                  </Link>
                </motion.div>
              ))}

              <motion.div
                className="mt-8 grid gap-3"
                initial={{ opacity: 0, y: 14 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: 0.34, duration: 0.4, ease: EASE }}
              >
                <ButtonLink href="/guide/installation" variant="primary" size="md">
                  Install CachyCLI
                </ButtonLink>
                <ButtonLink href={GITHUB} external variant="ghost" size="md">
                  GitHub ↗
                </ButtonLink>
              </motion.div>
            </nav>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

function CommandLine({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div>
      <p className="mono-sm mb-2" style={{ color: "var(--ink-38)" }}>
        {label}
      </p>
      <button
        type="button"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1800);
          } catch {
            /* clipboard unavailable */
          }
        }}
        className="code block w-full overflow-x-auto whitespace-nowrap px-3 py-3 text-left transition-colors duration-300"
        style={{
          background: "var(--bg-deep)",
          border: "1px solid var(--line)",
          color: copied ? "var(--accent)" : "var(--ink-60)",
        }}
      >
        {copied ? "copied ✓" : value}
      </button>
    </div>
  );
}

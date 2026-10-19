import type { ReactNode } from "react";

export function Container({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`mx-auto px-[var(--pad)] ${className}`} style={{ maxWidth: "var(--maxw)" }}>
      {children}
    </div>
  );
}

export type SectionTone = "dark" | "deep" | "paper";

const TONE: Record<SectionTone, string> = {
  dark: "",
  deep: "",
  paper: "paper-section",
};

const TONE_STYLE: Record<SectionTone, React.CSSProperties> = {
  dark: {},
  deep: { background: "var(--bg-deep)" },
  paper: {},
};

/** A page section with consistent vertical rhythm. */
export function Section({
  id,
  tone = "dark",
  className = "",
  children,
}: {
  id?: string;
  tone?: SectionTone;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section
      id={id}
      data-tone={tone}
      className={`scroll-mt-[88px] ${TONE[tone]} ${className}`}
      style={{ paddingBlock: "var(--section-y)", ...TONE_STYLE[tone] }}
    >
      {children}
    </section>
  );
}

/** Numbered mono label row: "03 / PROVIDERS" with an optional aside. */
export function SectionHead({
  index,
  title,
  aside,
  light = false,
}: {
  index: string;
  title: string;
  aside?: ReactNode;
  light?: boolean;
}) {
  return (
    <div
      className="flex flex-wrap items-baseline gap-x-6 gap-y-1 border-t pt-4"
      style={{ borderColor: light ? "rgba(25,26,31,0.16)" : "var(--line)" }}
    >
      <span className="mono-sm tnum" style={{ color: light ? "rgba(25,26,31,0.68)" : "var(--ink-38)" }}>
        {index} / {title}
      </span>
      {aside ? (
        <span
          className="mono-sm ml-auto text-right"
          style={{ color: light ? "rgba(25,26,31,0.68)" : "var(--ink-38)" }}
        >
          {aside}
        </span>
      ) : null}
    </div>
  );
}

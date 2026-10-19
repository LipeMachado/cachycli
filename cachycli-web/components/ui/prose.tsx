import Link from "next/link";
import Shot from "@/components/ui/shot";
import type { ReactNode } from "react";

/* ------------------------------------------------------------------ */
/*  Text                                                               */
/* ------------------------------------------------------------------ */

export function Kicker({ children }: { children: ReactNode }) {
  return (
    <span className="mono-sm" style={{ color: "var(--ink-38)" }}>
      {children}
    </span>
  );
}

export function H1({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <h1 className={`display h1 ${className}`}>{children}</h1>;
}

export function H2({
  children,
  className = "",
  id,
}: {
  children: ReactNode;
  className?: string;
  id?: string;
}) {
  return (
    <h2 id={id} className={`display h2 scroll-mt-[96px] ${className}`}>
      {children}
    </h2>
  );
}

export function H3({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <h3 className={`display h3 ${className}`}>{children}</h3>;
}

export function Lead({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <p className={`lede ${className}`}>{children}</p>;
}

export function Prose({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`space-y-4 text-[1rem] leading-relaxed ${className}`} style={{ color: "var(--ink-60)" }}>
      {children}
    </div>
  );
}

export function Strong({ children }: { children: ReactNode }) {
  return <span style={{ color: "var(--ink)" }}>{children}</span>;
}

export function Muted({ children }: { children: ReactNode }) {
  return (
    <span className="mono-sm" style={{ color: "var(--ink-38)" }}>
      {children}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/*  Blocks                                                             */
/* ------------------------------------------------------------------ */

export function Code({
  children,
  label,
  className = "",
}: {
  children: ReactNode;
  label?: string;
  className?: string;
}) {
  return (
    <div className={className}>
      {label ? (
        <p className="mono-sm mb-3" style={{ color: "var(--ink-38)" }}>
          {label}
        </p>
      ) : null}
      <pre className="codeblock">{children}</pre>
    </div>
  );
}

export function Table({
  head,
  rows,
}: {
  head: string[];
  rows: (ReactNode[] | string[])[];
}) {
  return (
    <table className="brutal">
      <thead>
        <tr>
          {head.map((cell) => (
            <th key={cell}>{cell}</th>
          ))}
        </tr>
      </thead>
      <tbody style={{ color: "var(--ink-60)" }}>
        {rows.map((row, i) => (
          <tr key={i}>
            {row.map((cell, j) => (
              <td key={j}>{cell}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function KeyChip({ children }: { children: ReactNode }) {
  return <kbd>{children}</kbd>;
}

export function KeyTable({ items }: { items: { key: string; action: string }[] }) {
  return (
    <div className="rule-t">
      {items.map((item) => (
        <div key={item.key} className="rule-b flex flex-wrap items-center gap-x-5 gap-y-1 py-3">
          <span className="min-w-[190px] flex-1">
            <KeyChip>{item.key}</KeyChip>
          </span>
          <span className="text-[0.92rem]" style={{ color: "var(--ink-60)" }}>
            {item.action}
          </span>
        </div>
      ))}
    </div>
  );
}

export function Steps({ items }: { items: { title: string; body: ReactNode }[] }) {
  return (
    <ol className="rule-t">
      {items.map((item, i) => (
        <li key={item.title} className="rule-b grid gap-2 py-5 sm:grid-cols-[64px_1fr]">
          <span className="mono tnum" style={{ color: "var(--accent)" }}>
            {String(i + 1).padStart(2, "0")}
          </span>
          <div>
            <p className="mono" style={{ color: "var(--ink)" }}>
              {item.title}
            </p>
            <div className="mt-2 text-[0.95rem]" style={{ color: "var(--ink-60)" }}>
              {item.body}
            </div>
          </div>
        </li>
      ))}
    </ol>
  );
}

export function Callout({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="panel panel-pad" style={{ borderLeft: "2px solid var(--accent)" }}>
      <p className="mono" style={{ color: "var(--ink)" }}>
        {title}
      </p>
      <div className="mt-3 text-[0.95rem]" style={{ color: "var(--ink-60)" }}>
        {children}
      </div>
    </div>
  );
}

/** Grid of link cards with identical geometry. */
export function CardGrid({
  items,
  columns = 3,
}: {
  items: { href: string; title: string; body: string; badge?: string }[];
  columns?: 2 | 3 | 4;
}) {
  const cols =
    columns === 2
      ? "sm:grid-cols-2"
      : columns === 3
        ? "sm:grid-cols-2 lg:grid-cols-3"
        : "sm:grid-cols-2 lg:grid-cols-4";
  return (
    <div className={`grid gap-4 ${cols}`}>
      {items.map((item) => (
        <Link key={item.href} href={item.href} className="panel panel-hover panel-pad group flex flex-col">
          <span className="mono flex items-baseline gap-3" style={{ color: "var(--ink)" }}>
            {item.badge ? (
              <span style={{ color: "var(--accent)" }}>{item.badge}</span>
            ) : null}
            {item.title}
          </span>
          <span className="mt-2 text-[0.9rem]" style={{ color: "var(--ink-60)" }}>
            {item.body}
          </span>
          <span
            className="mono-sm mt-4 inline-flex items-center gap-2 transition-transform duration-500 group-hover:translate-x-1"
            style={{ color: "var(--accent)" }}
          >
            Open →
          </span>
        </Link>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  Article (guide pages)                                              */
/* ------------------------------------------------------------------ */

export function Article({ children }: { children: ReactNode }) {
  return <div className="max-w-[880px] space-y-9">{children}</div>;
}

export function ArticleBlock({
  title,
  kicker,
  children,
}: {
  title: string;
  kicker?: string;
  children: ReactNode;
}) {
  return (
    <section>
      {kicker ? (
        <p className="mono-sm mb-3" style={{ color: "var(--ink-38)" }}>
          {kicker}
        </p>
      ) : null}
      <h2 className="display h3">{title}</h2>
      <div className="mt-5 space-y-5">{children}</div>
    </section>
  );
}

export function Bullets({ items }: { items: ReactNode[] }) {
  return (
    <ul className="space-y-2.5">
      {items.map((item, i) => (
        <li key={i} className="flex items-baseline gap-3 text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          <span style={{ color: "var(--accent)" }}>▪</span>
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

/** Screenshot in a terminal shell frame; optimised through next/image. */
export function ShellFrame({
  src,
  alt,
  caption,
  tag,
  width = 2817,
  height = 2068,
  ratio,
  priority = false,
  sizes = "(max-width: 640px) 100vw, (max-width: 1024px) 50vw, 620px",
  className = "",
}: {
  src: string;
  alt: string;
  caption: string;
  tag?: string;
  width?: number;
  height?: number;
  ratio?: string;
  priority?: boolean;
  sizes?: string;
  className?: string;
}) {
  const aspect = ratio ?? `${width} / ${height}`;
  return (
    <figure className={className}>
      <div className="shell overflow-hidden">
        <div className="shell-bar">
          <div className="shell-dots" aria-hidden="true">
            <i />
            <i />
            <i />
          </div>
          <span className="truncate">{caption}</span>
          {tag ? (
            <span className="ml-auto shrink-0" style={{ color: "var(--accent)" }}>
              {tag}
            </span>
          ) : null}
        </div>
        <div className="relative overflow-hidden" style={{ aspectRatio: aspect }}>
          <Shot
            src={src}
            alt={alt}
            width={width}
            height={height}
            sizes={sizes}
            priority={priority}
            quality={90}
            className="h-full w-full object-cover object-top"
          />
        </div>
      </div>
    </figure>
  );
}

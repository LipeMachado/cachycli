import Link from "next/link";
import { ButtonLink } from "@/components/ui/button";
import { H1, Lead } from "@/components/ui/prose";
import { Container } from "@/components/ui/section";

export default function NotFound() {
  return (
    <Container>
      <div
        className="flex min-h-[70vh] flex-col justify-center"
        style={{ paddingTop: "var(--nav-h)", paddingBlock: "clamp(48px,8vw,120px)" }}
      >
        <span className="mono-sm" style={{ color: "var(--accent)" }}>
          404
        </span>
        <H1 className="mt-5">
          This page does not
          <br />
          exist.
        </H1>
        <Lead className="mt-6">
          The link may be old, or the address may have a typo. Everything lives in the guide.
        </Lead>

        <div className="mt-8 flex flex-wrap gap-3">
          <ButtonLink href="/" size="lg">
            Back to the start
          </ButtonLink>
          <ButtonLink href="/guide" variant="ghost" size="lg">
            Open the guide
          </ButtonLink>
        </div>

        <div className="rule-t mt-12 grid gap-4 pt-6 sm:grid-cols-2 lg:grid-cols-4">
          {[
            { href: "/guide/installation", label: "Installation" },
            { href: "/guide/providers", label: "Providers" },
            { href: "/guide/mcp", label: "MCP servers" },
            { href: "/guide/reference", label: "CLI reference" },
          ].map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className="mono-sm transition-colors duration-300 hover:text-[var(--accent)]"
              style={{ color: "var(--ink-60)" }}
            >
              {item.label} →
            </Link>
          ))}
        </div>
      </div>
    </Container>
  );
}

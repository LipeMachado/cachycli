import Link from "next/link";
import Wordmark from "@/components/site/wordmark";
import { ButtonLink } from "@/components/ui/button";
import { Container } from "@/components/ui/section";
import { Logo } from "@/components/site/nav";
import { GITHUB, RELEASES } from "@/lib/content";

const GROUPS: { title: string; links: { label: string; href: string; external?: boolean }[] }[] = [
  {
    title: "Guide",
    links: [
      { label: "Installation", href: "/guide/installation" },
      { label: "Quick start", href: "/guide/quickstart" },
      { label: "Providers", href: "/guide/providers" },
      { label: "Slash commands", href: "/guide/commands" },
      { label: "Reference", href: "/guide/reference" },
    ],
  },
  {
    title: "More",
    links: [
      { label: "MCP servers", href: "/guide/mcp" },
      { label: "Editors", href: "/guide/acp" },
      { label: "Sessions", href: "/guide/sessions" },
      { label: "Troubleshooting", href: "/guide/troubleshooting" },
      { label: "Releases", href: RELEASES, external: true },
    ],
  },
];

export default function Footer() {
  return (
    <footer style={{ background: "var(--bg-deep)" }} className="rule-t">
      <Container>
        <div className="grid gap-12 pt-[clamp(44px,5vw,80px)] pb-10 lg:grid-cols-[1.1fr_1.4fr]">
          <div>
            <span style={{ color: "var(--ink)" }}>
              <Logo />
            </span>
            <p className="mt-4 max-w-[30ch] text-[0.98rem]" style={{ color: "var(--ink-60)" }}>
              A coding agent for your terminal. Drives opencode, MCP and your editors.
            </p>
            <div className="mt-6 flex flex-wrap gap-3">
              <ButtonLink href="/guide/installation" size="sm">
                Install
              </ButtonLink>
              <ButtonLink href={GITHUB} external variant="ghost" size="sm">
                GitHub ↗
              </ButtonLink>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-8 justify-items-start">
            {GROUPS.map((group) => (
              <div key={group.title} className="w-full text-left sm:max-w-[220px]">
                <p className="mono-sm" style={{ color: "var(--ink-38)" }}>
                  {group.title}
                </p>
                <ul className="mt-4 space-y-2.5">
                  {group.links.map((link) => (
                    <li key={link.label}>
                      {link.external ? (
                        <a
                          href={link.href}
                          target="_blank"
                          rel="noreferrer"
                          className="text-[0.92rem] transition-colors duration-300 hover:text-[var(--accent)]"
                          style={{ color: "var(--ink-60)" }}
                        >
                          {link.label}
                        </a>
                      ) : (
                        <Link
                          href={link.href}
                          className="text-[0.92rem] transition-colors duration-300 hover:text-[var(--accent)]"
                          style={{ color: "var(--ink-60)" }}
                        >
                          {link.label}
                        </Link>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </div>

        <Wordmark />

        <div className="flex justify-end py-6">
          <p className="mono-sm" style={{ color: "var(--ink-38)" }}>
            Made for the terminal
          </p>
        </div>
      </Container>
    </footer>
  );
}

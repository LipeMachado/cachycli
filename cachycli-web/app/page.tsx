import BackendSwitcher from "@/components/site/backend-switcher";
import Marquee from "@/components/site/marquee";
import TickRail from "@/components/site/tick-rail";
import OsInstall from "@/components/site/os-install";
import { Button, ButtonLink } from "@/components/ui/button";
import { CardGrid, H1, H2, H3, Lead, ShellFrame, Steps as StepsBlock } from "@/components/ui/prose";
import { Parallax, Reveal, ScrollProgress, Stagger } from "@/components/ui/reveal";
import { Container, Section, SectionHead } from "@/components/ui/section";
import { GITHUB, HIGHLIGHTS, STEPS, VERSION } from "@/lib/content";

const RAIL = [
  { id: "hero", index: "01", label: "Overview" },
  { id: "interface", index: "02", label: "Interface" },
  { id: "providers", index: "03", label: "Providers" },
  { id: "integrations", index: "04", label: "Integrations" },
  { id: "install", index: "05", label: "Install" },
  { id: "guide", index: "06", label: "Guide" },
];

export default function Home() {
  return (
    <>
      <ScrollProgress />
      <TickRail items={RAIL} />

      {/* 01 — hero ------------------------------------------------------ */}
      <Section id="hero" className="!pt-[calc(var(--nav-h)+clamp(24px,4.5vw,64px))] !pb-0">
        <Container>
          <div className="mt-[clamp(20px,3.5vw,48px)] grid items-start gap-10 lg:grid-cols-[1.02fr_1fr] lg:gap-x-10 lg:gap-y-0">
            {/* On phones the screenshot sits right under the headline, so it is
                already on screen without scrolling; on desktop it keeps the
                right column. */}
            <div className="lg:col-start-1 lg:row-start-1">
              <H1>
                One terminal.
                <br />
                <span style={{ color: "var(--accent)" }}>Every agent.</span>
              </H1>
            </div>

            <div className="lg:col-start-2 lg:row-span-2 lg:row-start-1">
              <Parallax distance={30}>
                <Reveal y={24} blur={0} amount={0.05}>
                  <ShellFrame
                    src="/shots/01-welcome.webp"
                    alt="CachyCLI welcome screen with the prompt box and shortcut hints"
                    caption="cachycli — first run"
                    tag={VERSION}
                    priority
                  />
                </Reveal>
              </Parallax>
            </div>

            <div className="lg:col-start-1 lg:row-start-2">
              <Lead className="mt-2 lg:mt-7">
                CachyCLI is a coding agent for your terminal. Bring your own model, plug into your
                editor, or let it drive opencode — same interface, same sessions.
              </Lead>

              <div className="mt-8 flex flex-wrap items-center gap-3">
                <ButtonLink href="/guide/installation" size="lg">
                  Install CachyCLI
                </ButtonLink>
                <ButtonLink href="/guide" variant="ghost" size="lg">
                  Read the guide
                </ButtonLink>
              </div>

              <p className="mono-sm mt-6" style={{ color: "var(--ink-38)" }}>
                Free · open source
              </p>
            </div>
          </div>
        </Container>

        <div className="mt-[clamp(40px,5vw,88px)]">
          <Marquee />
        </div>
      </Section>

      {/* 02 — interface ------------------------------------------------- */}
      <Section id="interface">
        <Container>
          <SectionHead index="02" title="THE INTERFACE" />

          <div className="mt-6 grid gap-10 lg:grid-cols-[0.85fr_1.35fr] lg:gap-14">
            <div>
              <H2>
                Clear answers,
                <br />
                not walls of text.
              </H2>
              <Stagger className="mt-8">
                {HIGHLIGHTS.map((item) => (
                  <div key={item.title} className="rule-t py-5">
                    <p className="mono" style={{ color: "var(--ink)" }}>
                      {item.title}
                    </p>
                    <p className="mt-2 text-[0.95rem]" style={{ color: "var(--ink-60)" }}>
                      {item.body}
                    </p>
                  </div>
                ))}
              </Stagger>
            </div>

            <div className="grid gap-6">
              <Reveal y={22} blur={0}>
                <ShellFrame
                  src="/shots/05-chat.webp"
                  alt="CachyCLI showing an edit with a diff, a short answer and a code block"
                  caption="tool call · diff · answer"
                  tag="TUI"
                />
              </Reveal>
              <div className="grid gap-6 sm:grid-cols-2">
                <Reveal y={18} blur={0} delay={0.04}>
                  <ShellFrame
                    src="/shots/02-approval-bash.webp"
                    alt="CachyCLI asking for approval before running a command"
                    caption="approval"
                    tag="GATE"
                  />
                </Reveal>
                <Reveal y={18} blur={0} delay={0.08}>
                  <ShellFrame
                    src="/shots/10-theme-modal.webp"
                    alt="CachyCLI theme picker"
                    caption="themes"
                    tag="UI"
                    ratio="1878 / 1045"
                  />
                </Reveal>
              </div>
            </div>
          </div>
        </Container>
      </Section>

      {/* 03 — backends (paper) ------------------------------------------ */}
      <Section id="providers" tone="paper" className="!py-0">
        <Container className="pt-[var(--section-y)]">
          <SectionHead index="03" title="PROVIDERS" light />
          <div className="mt-6 grid gap-6 lg:grid-cols-[1.1fr_0.9fr] lg:items-end">
            <H2>
              Three ways in.
              <br />
              One prompt box.
            </H2>
            <p className="statement">Switch whenever you like — mid-session included.</p>
          </div>
        </Container>

        <Container className="mt-8 lg:mt-10">
          <BackendSwitcher />
        </Container>
      </Section>

      {/* 04 — integrations ---------------------------------------------- */}
      <Section id="integrations">
        <Container>
          <SectionHead index="04" title="INTEGRATIONS" />

          <H2 className="mt-6 max-w-[20ch]">
            CachyCLI drives agents.
            <br />
            Agents drive CachyCLI.
          </H2>

          <div className="mt-7 grid gap-6 lg:grid-cols-2">
            <Reveal blur={0}>
              <div className="panel panel-pad h-full">
                <p className="mono" style={{ color: "var(--accent)" }}>
                  Use your opencode
                </p>
                <H3 className="mt-4">Keep the tool you already run.</H3>
                <p className="mt-3 text-[0.98rem]" style={{ color: "var(--ink-60)" }}>
                  Models, logins and permissions stay with opencode. CachyCLI is the interface in
                  front of it.
                </p>
                <pre className="codeblock mt-6">cachycli --backend opencode</pre>
              </div>
            </Reveal>

            <Reveal blur={0} delay={0.05}>
              <div className="panel panel-pad h-full">
                <p className="mono" style={{ color: "var(--accent)" }}>
                  Add CachyCLI to your tools
                </p>
                <H3 className="mt-4">One command to plug it in.</H3>
                <p className="mt-3 text-[0.98rem]" style={{ color: "var(--ink-60)" }}>
                  Other agents and editors can call CachyCLI as a tool server — the same
                  sessions, the same projects.
                </p>
                <pre className="codeblock mt-6">cachycli mcp serve</pre>
              </div>
            </Reveal>
          </div>
        </Container>
      </Section>

      {/* 05 — install --------------------------------------------------- */}
      <Section id="install" tone="deep">
        <Container>
          <SectionHead index="05" title="INSTALL" />

          <div className="mt-6 grid gap-10 lg:grid-cols-[0.8fr_1.2fr] lg:gap-14">
            <div>
              <H2>
                Ready in
                <br />
                three steps.
              </H2>
              <StepsBlock
                items={STEPS.map((step) => ({
                  title: step.title,
                  body: step.body,
                }))}
              />
            </div>

            <div className="grid content-start gap-6">
              <OsInstall />
              <div className="grid gap-6 sm:grid-cols-2">
                <Reveal blur={0}>
                  <ShellFrame
                    src="/shots/09-settings.webp"
                    alt="CachyCLI settings"
                    caption="settings"
                    tag="F2"
                  />
                </Reveal>
                <Reveal blur={0} delay={0.05}>
                  <ShellFrame
                    src="/shots/07-palette.webp"
                    alt="CachyCLI command palette"
                    caption="palette"
                    tag="CTRL+P"
                  />
                </Reveal>
              </div>
            </div>
          </div>
        </Container>
      </Section>

      {/* 06 — guide ----------------------------------------------------- */}
      <Section id="guide">
        <Container>
          <SectionHead index="06" title="THE GUIDE" />

          <div className="mt-6 grid gap-10 lg:grid-cols-[1fr_1.1fr] lg:items-center">
            <div>
              <p className="statement max-w-[26ch]">
                “Everything you need to install it, connect it and use it.”
              </p>
              <div className="mt-8 flex flex-wrap gap-3">
                <ButtonLink href="/guide/installation" variant="amber" size="lg">
                  Install now
                </ButtonLink>
                <ButtonLink href="/guide" variant="ghost" size="lg">
                  Browse the guide
                </ButtonLink>
              </div>
            </div>

            <CardGrid
              columns={2}
              items={[
                { href: "/guide/installation", title: "Installation", body: "Install and update it." },
                { href: "/guide/quickstart", title: "Quick start", body: "Your first session." },
                { href: "/guide/providers", title: "Providers", body: "opencode, Claude Code, models." },
                { href: "/guide/mcp", title: "MCP", body: "Connect tool servers." },
                { href: "/guide/interface", title: "Interface", body: "Shortcuts and panels." },
                { href: "/guide/reference", title: "Reference", body: "Every command and flag." },
              ]}
            />
          </div>
        </Container>
      </Section>
    </>
  );
}

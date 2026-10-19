import type { Metadata } from "next";
import OsInstall from "@/components/site/os-install";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted, Steps } from "@/components/ui/prose";
import { Reveal } from "@/components/ui/reveal";
import { ShellFrame } from "@/components/ui/prose";

export const metadata: Metadata = {
  title: "Installation",
  description: "Install CachyCLI on Linux, macOS or Windows, update it and check that it works.",
};

export default function InstallationPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 02</Muted>
        <H1 className="mt-4">Installation</H1>
        <Lead className="mt-6">
          One command for your system. The installer picks the right download, puts the program in
          your home folder and adds it to your PATH.
        </Lead>
      </header>

      <Reveal blur={0}>
        <OsInstall />
      </Reveal>

      <ArticleBlock title="Check that it works">
        <Code label="In a new terminal">{`$ cachycli version
$ cachycli doctor`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          <code className="code">doctor</code> prints a short list: terminal support, whether a model
          is reachable, and whether opencode or tool servers were found.
        </p>
      </ArticleBlock>

      <ArticleBlock title="From source" kicker="Prefer to build it?">
        <Code>{`$ cargo install --git https://github.com/LipeMachado/cachycli cachycli`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Any platform with a current Rust toolchain works. The binary lands in your cargo bin
          folder, which is usually already on your PATH.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Update it">
        <Code>{`$ cachycli update --check    # is there something newer?
$ cachycli update            # install it`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Where things live">
        <Bullets
          items={[
            <>
              The program sits in <code className="code">~/.cachy/bin</code> and is added to your
              shell profile by the installer.
            </>,
            <>
              Settings, keys and conversations live in <code className="code">~/.cachy</code> — plain
              files you can back up or remove.
            </>,
            <>
              Nothing is uploaded anywhere. Removing the folder removes everything CachyCLI knows.
            </>,
          ]}
        />
        <Callout title="Uninstalling">
          Delete <code className="code">~/.cachy</code> and the PATH line the installer added to your
          shell profile. That is the whole uninstall.
        </Callout>
      </ArticleBlock>

      <ArticleBlock title="First run" kicker="Connect something to answer your prompts">
        <Code label="Inside the interface">{`/provider          pick opencode, Claude Code or your own models`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          If a provider needs a sign-in, CachyCLI steps aside, runs it — the browser opens where it
          should — and comes back when you are done.
        </p>
      </ArticleBlock>

      <ArticleBlock title="After installing">
        <Steps
          items={[
            {
              title: "Open CachyCLI",
              body: (
                <>
                  Run <code className="code">cachycli</code>. The first screen shows where you are
                  and what it can reach.
                </>
              ),
            },
            {
              title: "Pick a backend",
              body: (
                <>
                  If opencode is installed, it is already an option — see{" "}
                  <a href="/guide/providers" className="link-underline" style={{ color: "var(--ink)" }}>
                    Providers
                  </a>
                  .
                </>
              ),
            },
            {
              title: "Ask something",
              body: "Type a request and press Enter. The first reply is the quickest way to see how it behaves.",
            },
          ]}
        />
      </ArticleBlock>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/01-welcome.webp"
          alt="The CachyCLI welcome screen"
          caption="after install"
          tag="FIRST RUN"
        />
      </Reveal>
    </Article>
  );
}

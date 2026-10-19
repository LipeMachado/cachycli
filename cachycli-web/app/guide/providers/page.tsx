import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted, ShellFrame, Table } from "@/components/ui/prose";
import { Reveal } from "@/components/ui/reveal";

export const metadata: Metadata = {
  title: "Providers",
  description: "Connect opencode, Claude Code, your own models or an editor agent to CachyCLI.",
};

export default function ProvidersPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 04</Muted>
        <H1 className="mt-4">Providers</H1>
        <Lead className="mt-6">
          A provider is where your messages go. CachyCLI picks a sensible one on its own, and you
          can change it any time by typing <code className="code">/provider</code> inside the
          interface.
        </Lead>
      </header>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/12-providers.webp"
          alt="CachyCLI provider picker listing opencode, Claude Code, your models and an editor agent"
          caption="choosing a provider"
          tag="/PROVIDER"
          width={1878}
          height={787}
          ratio="1878 / 787"
        />
      </Reveal>

      <ArticleBlock title="The list">
        <Table
          head={["Provider", "What it uses", "Sign-in"]}
          rows={[
            ["opencode", "Your opencode install — its models, logins and tools", "opens the opencode sign-in"],
            ["Claude Code", "The Claude Code CLI on this machine", "runs Claude Code so you can log in"],
            ["Your models", "Any endpoint you configure (hosted or local)", "cachycli login --api-key …"],
            ["Editor agent", "An agent that speaks the editor standard", "handled by that agent"],
          ]}
        />
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          The picker shows the state of each one, so you always know what is ready and what still
          needs a sign-in.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Connect opencode" kicker="Recommended if you already use it">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Nothing to configure: CachyCLI uses your opencode — same logins, same models, same tools.
          If you are not signed in yet, choosing it starts the opencode sign-in, which opens your
          browser.
        </p>
        <Code label="Inside the interface">{`/provider          then pick "opencode"`}</Code>
        <Code label="Or make it the default">{`[agent]
provider = "opencode"`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Connect Claude Code">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          CachyCLI can drive the Claude Code CLI: your existing subscription, its tools, its
          permissions. Choose <span style={{ color: "var(--ink)" }}>Claude Code</span> in the picker;
          if it is not signed in, CachyCLI steps aside, runs Claude Code so you can finish the login,
          and comes back.
        </p>
        <Code label="Requirements">{`# the Claude Code CLI must be installed
npm install -g @anthropic-ai/claude-code

# then, inside CachyCLI
/provider          then pick "Claude Code"`}</Code>
        <Callout title="Switching is per session">
          Changing provider does not delete anything: your CachyCLI sessions stay where they are, and
          the next message simply goes to the new provider.
        </Callout>
      </ArticleBlock>

      <ArticleBlock title="Use your own models">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Add an endpoint once and it appears in the model picker — a hosted API or something running
          on your machine.
        </p>
        <Code>{`[model.my-model]
model = "llama-3.1-70b"
base_url = "http://localhost:8080/v1"
name = "Local Llama"`}</Code>
        <Bullets
          items={[
            <>
              Keys and endpoints: <a href="/guide/models" className="link-underline" style={{ color: "var(--ink)" }}>Models &amp; keys</a>.
            </>,
            "Switch model at any time with /model inside the interface.",
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Pick one for a single run">
        <Code>{`$ cachycli --provider opencode
$ cachycli --provider claude-code -p "summarize this diff"

# remember a choice
[agent]
provider = "opencode"`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Run without any of this and CachyCLI uses the first provider that is installed and signed
          in — usually opencode.
        </p>
      </ArticleBlock>
    </Article>
  );
}

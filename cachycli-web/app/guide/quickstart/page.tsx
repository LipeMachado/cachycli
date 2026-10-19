import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Code, H1, Lead, Muted, ShellFrame } from "@/components/ui/prose";
import { Reveal } from "@/components/ui/reveal";

export const metadata: Metadata = {
  title: "Quick start",
  description: "Your first CachyCLI session, headless runs and named sessions.",
};

export default function QuickStartPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 03</Muted>
        <H1 className="mt-4">Quick start</H1>
        <Lead className="mt-6">
          Open it in a project, ask for something, approve what you agree with. Everything below
          works the same on every platform.
        </Lead>
      </header>

      <ArticleBlock title="Your first session">
        <Code label="In your project folder">{`$ cd ~/projects/my-app
$ cachycli`}</Code>
        <Bullets
          items={[
            <>First time here? Type <code className="code">/provider</code> and pick where your messages should go.</>,
            "Type what you want and press Enter.",
            "It reads files, runs commands and edits code — asking before anything that matters.",
            "Ctrl+C stops a turn. Ctrl+Q quits.",
          ]}
        />
      </ArticleBlock>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/05-chat.webp"
          alt="A CachyCLI session with an edit, a diff and an answer"
          caption="a turn"
          tag="TUI"
        />
      </Reveal>

      <ArticleBlock title="Attach files">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Type <code className="code">@</code> to pick a file, <code className="code">@src/</code> to
          browse a folder, <code className="code">@src/main.rs:10-50</code> for a range, or{" "}
          <code className="code">@!.env</code> for files that are normally hidden.
        </p>
        <Code>{`summarize @src/agent.rs:1-60 and suggest one improvement`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Do something without opening the interface" kicker="For scripts and pipelines">
        <Code label="Plain text, JSON, or streaming">{`$ cachycli -p "review the staged diff"
$ cachycli -p "list the TODOs" --output-format json | jq -r .text
$ git diff | cachycli -p "write a commit message for this"`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Keep context between runs">
        <Code label="Named sessions">{`$ cachycli -p "remember: the deploy key is in .env" -s nightly
$ cachycli -p "what did I tell you about the key?" -s nightly

$ cachycli -c              # continue the last session here
$ cachycli --resume nightly  # by name
$ cachycli --resume         # pick from a list`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Give it project rules" kicker="Optional, but it pays off">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Create an <code className="code">AGENTS.md</code> in the repository and CachyCLI follows it
          in every session there — build commands, style, anything you keep repeating.
        </p>
        <Code label="AGENTS.md">{`# Project rules

- Use pnpm, never npm.
- Run \`pnpm test\` before saying a change is done.
- Keep comments in English.`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Next">
        <Bullets
          items={[
            <>
              Pick how it should think: <a href="/guide/providers" className="link-underline" style={{ color: "var(--ink)" }}>Providers</a>.
            </>,
            <>
              Learn the keys and panels: <a href="/guide/interface" className="link-underline" style={{ color: "var(--ink)" }}>Interface</a>.
            </>,
            <>
              Control what it may do: <a href="/guide/tools" className="link-underline" style={{ color: "var(--ink)" }}>Tools &amp; permissions</a>.
            </>,
          ]}
        />
      </ArticleBlock>
    </Article>
  );
}

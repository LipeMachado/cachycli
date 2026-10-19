import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Code, H1, KeyTable, Lead, Muted, ShellFrame } from "@/components/ui/prose";
import { Reveal } from "@/components/ui/reveal";
import { SHORTCUTS } from "@/lib/content";

export const metadata: Metadata = {
  title: "Interface & shortcuts",
  description: "The prompt box, panels, modals and every keyboard shortcut in CachyCLI.",
};

export default function InterfacePage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 06</Muted>
        <H1 className="mt-4">Interface &amp; shortcuts</H1>
        <Lead className="mt-6">
          One screen: the conversation, a prompt box, and panels you open when you need them. It is
          built to be driven from the keyboard.
        </Lead>
      </header>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/01-welcome.webp"
          alt="The CachyCLI welcome screen with the prompt box and hints"
          caption="welcome"
          tag="FIRST RUN"
        />
      </Reveal>

      <ArticleBlock title="What is on screen">
        <Bullets
          items={[
            "Top line: where you are, how much of the model's context is used, and the dashboard.",
            "Middle: the conversation — your messages, the agent's answers, tool calls and diffs.",
            "Bottom: the prompt box with the current model on the right, and the key hints below.",
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Keyboard shortcuts" kicker="Every key, in one place">
        <KeyTable items={SHORTCUTS} />
      </ArticleBlock>

      <ArticleBlock title="Panels and modals">
        <Code label="Open them by key">{`/provider pick where your messages go
Ctrl+P   command palette — every action in one list
Ctrl+X   keyboard shortcuts
F2       settings — theme, panel visibility, defaults
Ctrl+R   resume a session
Ctrl+T   task panel for the current plan
/todos   same panel, from the prompt
/mode    Normal, Plan or Always-approve
/mouse   hand the mouse back to the terminal`}</Code>
      </ArticleBlock>

      <ArticleBlock title="While a turn is running">
        <Bullets
          items={[
            "Tool calls appear as compact blocks: running, done or failed, with how long they took.",
            "Edits arrive as a diff so you see exactly what changed.",
            "Ctrl+C or Esc stops the turn; nothing else is cancelled.",
            "Walk away: `/notify bell` (or a desktop notification) tells you when a long turn ends, and `/timeline` shows where the time went.",
          ]}
        />
      </ArticleBlock>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/03-approval-edit.webp"
          alt="CachyCLI asking whether an edit should be allowed"
          caption="approval"
          tag="GATE"
        />
      </Reveal>

      <ArticleBlock title="Themes">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Five themes ship with it, from a dark default to a light one for daylight. Change it in
          settings or straight from the prompt.
        </p>
        <Code>{`/theme`}</Code>
      </ArticleBlock>
    </Article>
  );
}

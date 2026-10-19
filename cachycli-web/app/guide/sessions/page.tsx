import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Code, H1, Lead, Muted, ShellFrame } from "@/components/ui/prose";
import { Reveal } from "@/components/ui/reveal";

export const metadata: Metadata = {
  title: "Sessions",
  description: "Resume, rename, export and inspect your CachyCLI sessions.",
};

export default function SessionsPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 11</Muted>
        <H1 className="mt-4">Sessions</H1>
        <Lead className="mt-6">
          Every conversation is saved as you go, one folder per project, so you can close the
          terminal and pick up exactly where you stopped.
        </Lead>
      </header>

      <ArticleBlock title="Continue where you left off">
        <Code>{`$ cachycli -c                 # most recent session here
$ cachycli --resume           # choose from a list
$ cachycli --resume nightly   # by name
$ cachycli --resume 01a149…   # by id`}</Code>
        <Bullets
          items={[
            "Names are taken from your first message; rename them to something memorable.",
            "Matching ignores case and dashes, so fix-login-bug finds “Fix login bug”.",
            "Resuming continues the provider's own conversation too: opencode, ACP and Claude Code pick up where that session left off, and /new starts over on both sides.",
          ]}
        />
      </ArticleBlock>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/07-palette.webp"
          alt="CachyCLI session and command list"
          caption="find an old session"
          tag="CTRL+P"
        />
      </Reveal>

      <ArticleBlock title="Manage them">
        <Code>{`$ cachycli sessions list
$ cachycli sessions search payment
$ cachycli sessions restore nightly   # prints the command to reopen it
$ cachycli sessions delete old-thing

/rename payment work     inside the interface
/delete                  remove the current one`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Take a conversation with you">
        <Code>{`$ cachycli export                    # Markdown, next to you
$ cachycli export --output notes.md
$ cachycli usage                     # tokens and turns for a session`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Exports include your messages, the answers, the tool calls and the diffs — useful for a
          pull request or a hand-over.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Named sessions for scripts">
        <Code>{`$ cachycli -p "start the release check" -s release
$ cachycli -p "and the changelog?" -s release`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Named sessions are created on first use and reused after that — handy in CI or a nightly
          script.
        </p>
      </ArticleBlock>
    </Article>
  );
}

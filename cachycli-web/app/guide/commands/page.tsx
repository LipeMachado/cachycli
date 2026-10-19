import type { Metadata } from "next";
import { Article, ArticleBlock, Code, H1, Lead, Muted, ShellFrame, Table } from "@/components/ui/prose";
import { Reveal } from "@/components/ui/reveal";
import { SLASH_COMMANDS } from "@/lib/content";

export const metadata: Metadata = {
  title: "Slash commands",
  description: "Every command you can type after a slash inside CachyCLI.",
};

export default function CommandsPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 07</Muted>
        <H1 className="mt-4">Slash commands</H1>
        <Lead className="mt-6">
          Type <code className="code">/</code> in the prompt box and the list appears, filtered as
          you write. Common commands have short aliases.
        </Lead>
      </header>

      <Reveal blur={0}>
        <ShellFrame
          src="/shots/06-slash.webp"
          alt="The CachyCLI slash command menu"
          caption="command menu"
          tag="SLASH"
        />
      </Reveal>

      <Table
        head={["Command", "What it does"]}
        rows={SLASH_COMMANDS.map((command) => [
          <code className="code" key={command.command}>
            {command.command}
          </code>,
          command.description,
        ])}
      />

      <ArticleBlock title="Examples">
        <Code>{`/model my-model            switch model
/resume                    pick a past session
/rename payment work       name the current one
/export notes.md           save the whole conversation
/remember always run tests before committing
/compact                   free up context on a long chat`}</Code>
      </ArticleBlock>
    </Article>
  );
}

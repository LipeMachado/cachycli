import type { Metadata } from "next";
import { Article, ArticleBlock, Code, H1, Lead, Muted, Table } from "@/components/ui/prose";
import { CLI_FLAGS, SUBCOMMANDS } from "@/lib/content";

export const metadata: Metadata = {
  title: "CLI reference",
  description: "Every CachyCLI flag and subcommand, with examples.",
};

export default function ReferencePage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 13</Muted>
        <H1 className="mt-4">CLI reference</H1>
        <Lead className="mt-6">
          The complete list. Run <code className="code">cachycli --help</code> for the same thing in
          your terminal.
        </Lead>
      </header>

      <ArticleBlock title="Common examples">
        <Code>{`$ cachycli                              open the interface here
$ cachycli "fix the failing test"        open with a first request
$ cachycli -p "review this diff"         answer once and exit
$ cachycli -c                            continue the last session
$ cachycli --provider opencode           use your opencode
$ cachycli --provider claude-code        use Claude Code
$ cachycli agent stdio                   let an editor connect`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Flags">
        <Table
          head={["Flag", "What it does"]}
          rows={CLI_FLAGS.map((flag) => [
            <code className="code" key={flag.flag}>
              {flag.flag}
            </code>,
            flag.description,
          ])}
        />
      </ArticleBlock>

      <ArticleBlock title="Subcommands">
        <Table
          head={["Command", "What it does"]}
          rows={SUBCOMMANDS.map((command) => [
            <code className="code" key={command.name}>
              cachycli {command.name}
            </code>,
            command.description,
          ])}
        />
      </ArticleBlock>

      <ArticleBlock title="Output formats">
        <Table
          head={["Format", "Shape"]}
          rows={[
            [<code className="code" key="1">plain</code>, "Just the answer, for humans"],
            [<code className="code" key="2">json</code>, "One object at the end — text, session id, stop reason"],
            [<code className="code" key="3">streaming-json</code>, "One line per event as it happens"],
            [<code className="code" key="4">streaming-messages-json</code>, "The same stream in the widely used messages format"],
          ]}
        />
        <Code>{`$ cachycli -p "list the TODOs" --output-format json | jq -r .text`}</Code>
      </ArticleBlock>
    </Article>
  );
}

import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted, Table } from "@/components/ui/prose";
import { TOOLS } from "@/lib/content";

export const metadata: Metadata = {
  title: "Tools & permissions",
  description: "What the agent can do, and how to allow, block or approve it.",
};

export default function ToolsPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 08</Muted>
        <H1 className="mt-4">Tools &amp; permissions</H1>
        <Lead className="mt-6">
          The agent can read files, search, edit, run commands and look things up online. Everything
          that could change something asks first, unless you tell it otherwise.
        </Lead>
      </header>

      <ArticleBlock title="The four answers" kicker="When it asks">
        <Bullets
          items={[
            "Yes, and stop asking — for the rest of the session.",
            "Yes, allow this kind of change from now on.",
            "Yes, just this once.",
            "No — and you can type a short reason; it will not try the same thing again.",
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="What it can do">
        <Table
          head={["Tool", "In plain words"]}
          rows={TOOLS.map((tool) => [<code className="code" key={tool.name}>{tool.name}</code>, tool.summary])}
        />
      </ArticleBlock>

      <ArticleBlock title="Allow or block specific things" kicker="Rules, set once">
        <Code label="On the command line">{`$ cachycli --deny "Bash(rm*)"
$ cachycli --allow "Bash(npm*)" --deny "Bash(sudo*)"
$ cachycli --deny "Edit(/etc/**)"
$ cachycli --allow "WebFetch(domain:docs.rs)"`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Patterns are simple: <code className="code">*</code> for one part,{" "}
          <code className="code">**</code> for everything below a folder. A block always wins over an
          allow.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Modes" kicker="How much it may do unattended">
        <Table
          head={["Mode", "Behaviour"]}
          rows={[
            [<code className="code" key="a">default</code>, "Read-only work runs; edits and commands ask."],
            [<code className="code" key="b">acceptEdits</code>, "Edits go through; commands still ask."],
            [<code className="code" key="c">auto</code>, "Everything runs without asking."],
            [<code className="code" key="d">plan</code>, "Read-only session — useful to think first."],
            [<code className="code" key="e">dontAsk</code>, "Never interrupts; anything unapproved simply fails."],
          ]}
        />
        <Code>{`$ cachycli --permission-mode plan
/always-approve            toggle it inside a session`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Limit which tools exist">
        <Code>{`$ cachycli --tools "read_file,grep,list_dir"      # only these
$ cachycli --disallowed-tools "web_fetch,task"    # not these`}</Code>
        <Callout title="Safer defaults for a shared machine">
          Keep the default mode, add <code className="code">--deny</code> for the few commands your
          team never wants, and let everything else ask.
        </Callout>
      </ArticleBlock>

      <ArticleBlock title="Side tasks">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          For a well-scoped job the agent can start a helper with its own context — good for
          searching a large project without filling your chat. Helpers can be limited to reading
          only, and long ones keep running in the background while you continue.
        </p>
      </ArticleBlock>
    </Article>
  );
}

import type { Metadata } from "next";
import { Article, ArticleBlock, Code, H1, Lead, Muted, Table } from "@/components/ui/prose";

export const metadata: Metadata = {
  title: "Agents & jobs",
  description:
    "Agent profiles, goals with verification, multi-step workflows and the subagent limits.",
};

export default function AgentsPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 15</Muted>
        <H1 className="mt-4">Agents &amp; jobs</H1>
        <Lead className="mt-6">
          Beyond one prompt and one answer: profiles that shape the agent, a goal it keeps in
          view, verification passes, and multi-step jobs that run as subagents.
        </Lead>
      </header>

      <ArticleBlock title="Agent profiles" kicker="/personas">
        <p>
          A profile is a Markdown file that gets injected into the system prompt. Drop one in{" "}
          <code className="code">.cachy/agents/</code> (project),{" "}
          <code className="code">~/.cachy/agents/</code> (yours) or{" "}
          <code className="code">.claude/agents/</code> — the Claude Code layout also works.
          Frontmatter is optional.
        </p>
        <Code label=".cachy/agents/reviewer.md">{`---
name: reviewer
description: Reviews code before anything is written
---

You are in review mode.
- Read before you write.
- Prefer small, focused diffs.
- End every answer with a short "Risks" list.`}</Code>
        <p>
          Then <code className="code">/personas</code> lists them (with{" "}
          <code className="code">none</code> to go back to the default) and{" "}
          <code className="code">/persona reviewer</code> applies one directly. The choice is
          remembered in <code className="code">[agent] name</code>.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Goals, and proving them" kicker="/goal">
        <p>
          <code className="code">/goal ship the reviewer</code> records what the session is trying
          to achieve. The goal rides in the header, goes into every system prompt inside{" "}
          <code className="code">&lt;goal&gt;</code>, and is stored with the session — resuming it
          brings the goal back.
        </p>
        <p>
          <code className="code">/goal verify</code> then runs{" "}
          <code className="code">[agent] goal_verifier_count</code> (3 by default) verifier
          subagents in parallel. Each one reads the repository and answers{" "}
          <strong>met / not met / unclear</strong>, what it checked, and what is still missing. The
          reports land in the transcript and in <code className="code">/tasks</code>.
        </p>
        <p>
          The agent can do this itself too: the <code className="code">set_goal</code> tool lets the
          model record or clear the goal (and optionally verify it) when a request has several
          phases.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Workflows" kicker="/workflow">
        <p>
          A workflow is a named list of steps; every step runs as its own subagent, at most{" "}
          <code className="code">[subagents] max_concurrent</code> at a time, each one visible in{" "}
          <code className="code">/tasks</code>.
        </p>
        <Code label=".cachy/workflows/review.md">{`---
name: review
description: Two passes over the working tree
---

## Step: look
List the files that changed and say what each change does.

## Step: judge
List the risks and what could break.`}</Code>
        <p>
          Run it with <code className="code">/workflow review</code>, or just ask in plain language:
          the model has a <code className="code">run_workflow</code> tool and will use it when a task
          has clearly separable phases. A JSON form is also accepted:
        </p>
        <Code label=".cachy/workflows/ship.json">{`{"name":"ship","description":"Cut a release",
 "steps":[{"title":"test","prompt":"Run the tests"},
          {"title":"tag","prompt":"Tag the release"}]}`}</Code>
        <p>
          The agent can also <strong>write the plan itself</strong>: the{" "}
          <code className="code">plan_workflow</code> tool takes up to 8 steps with optional{" "}
          <code className="code">depends_on</code>, runs the independent ones together, and hands each
          dependent step the reports of the steps it needs — so &ldquo;investigate → change →
          verify&rdquo; actually builds on itself. With <code className="code">save: true</code> the
          plan is stored under <code className="code">.cachy/workflows/</code> and becomes reusable
          with <code className="code">/workflow</code>.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Limits you control" kicker="[subagents]">
        <Table
          head={["Setting", "What it does"]}
          rows={[
            [
              <code className="code" key="c">
                max_concurrent
              </code>,
              "How many subagents (workflow steps, verifiers, tasks) run at once. Extras queue.",
            ],
            [
              <code className="code" key="d">
                max_depth
              </code>,
              "How deep subagents may nest. Past it, the task tool refuses to delegate again.",
            ],
            [
              <code className="code" key="g">
                goal_verifier_count
              </code>,
              "How many verifiers /goal verify launches.",
            ],
            [
              <code className="code" key="e">
                auto_compact_threshold_percent
              </code>,
              "When the context passes this, the older segments are summarised automatically.",
            ],
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Long sessions" kicker="context">
        <p>
          <code className="code">/compact</code> summarises the conversation in segments: the older
          blocks are summarised one by one (nothing is dropped blindly) while the most recent
          messages stay verbatim. The result is written to the session too, so{" "}
          <code className="code">/resume</code> sees the same picture.
        </p>
        <p>
          <code className="code">/rewind 2</code> drops the last two turns,{" "}
          <code className="code">/fork</code> copies the session into a new one,{" "}
          <code className="code">/timeline</code> lists the turns with their durations,{" "}
          <code className="code">/jump</code> scrolls back to an earlier turn and{" "}
          <code className="code">/search text</code> looks through every saved session.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Picking a fast model" kicker="/bench">
        <p>
          With several providers and dozens of models, guessing is expensive.{" "}
          <code className="code">/bench</code> times the first token of each model on{" "}
          <em>your</em> machine, stores the results in{" "}
          <code className="code">~/.cachy/bench.json</code> and annotates the model list — so{" "}
          <code className="code">/model</code> shows <code className="code">· free, no key · 11.4s</code>{" "}
          instead of a name you have to trust.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Watching the work" kicker="/tasks">
        <p>
          Everything that runs in the background — shell commands, workflow steps, verifier
          subagents, plans — shows up in <code className="code">/tasks</code>: id, kind, elapsed
          time, a moving bar while it runs, and its status. <code className="code">Enter</code> opens
          a task&rsquo;s full output, <code className="code">x</code> kills it and{" "}
          <code className="code">r</code> refreshes; the panel also refreshes itself while it is
          open.
        </p>
        <p>
          <code className="code">/timeline</code> shows the turns of the session with a bar scaled to
          each duration, so you can see where the time went;{" "}
          <code className="code">/timeline all</code> does it across every session — when each turn
          ran, how long it took, how much work was in it and which project it was in — read back
          from the stored update streams, so it survives restarts.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Sharing and serving" kicker="/share · agent serve">
        <p>
          <code className="code">/share</code> writes the session to a portable{" "}
          <code className="code">.cachy</code> file (a readable JSON bundle with the messages, the
          model and the goal) — send it however you like; the other side opens it with{" "}
          <code className="code">/import &lt;file&gt;</code> and continues the conversation.
        </p>
        <p>
          The same bundle travels over a socket: <code className="code">cachycli share --to ws://host:8765</code>{" "}
          sends this directory&rsquo;s latest session to a running <code className="code">agent serve</code>,{" "}
          <code className="code">--id &lt;id&gt;</code> pulls one back and <code className="code">--list</code>{" "}
          shows what the server holds, and <code className="code">--remove &lt;id&gt;</code> makes it
          forget one. In the interface it is{" "}
          <code className="code">/share push</code>, <code className="code">/share pull &lt;id&gt;</code>{" "}
          and <code className="code">/shares</code> (with{" "}
          <code className="code">/shares remove &lt;id&gt;</code>);
          set <code className="code">[share] server</code> once and the url can be left out. The server keeps
          the bundles under its own <code className="code">~/.cachy/shares</code>.
        </p>
        <Code label="terminal">{`cachycli agent serve --port 8765   # WebSocket server, localhost by default
# client → {"prompt": "run the tests"}
# server → {"type":"text","data":"…"} … {"type":"done","stop":"end_turn"}
# client → {"cancel": true}`}</Code>
        <p>
          One agent per connection, in the server&rsquo;s directory, with the same events the
          interface shows. Nothing listens on the network unless you run it.
        </p>
        <p>
          Plugins can come from an index instead of a folder: point{" "}
          <code className="code">[plugins] index</code> at a JSON file or URL and{" "}
          <code className="code">/plugin search &lt;term&gt;</code> lists what matches; installing
          clones (git) or copies (path) the entry into{" "}
          <code className="code">~/.cachy/plugins/</code>.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Worktrees" kicker="/worktree">
        <p>
          <code className="code">/worktree</code> lists the git worktrees,{" "}
          <code className="code">/worktree fix-login</code> creates{" "}
          <code className="code">../&lt;repo&gt;-fix-login</code> on a new branch, and{" "}
          <code className="code">/worktree remove fix-login</code> deletes it. Handy to let one job
          work in its own checkout.
        </p>
      </ArticleBlock>
    </Article>
  );
}

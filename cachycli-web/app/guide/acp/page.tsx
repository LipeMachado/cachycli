import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted } from "@/components/ui/prose";

export const metadata: Metadata = {
  title: "Editors (ACP)",
  description: "Use CachyCLI inside Zed, Neovim, Emacs and other editors — or use your editor's agent in the terminal.",
};

export default function AcpPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 10</Muted>
        <H1 className="mt-4">Editors</H1>
        <Lead className="mt-6">
          CachyCLI speaks the same language editors use to run agents. That works in both
          directions: your editor can run CachyCLI, or CachyCLI can run the agent your editor
          already uses.
        </Lead>
      </header>

      <ArticleBlock title="Run CachyCLI in your editor">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Point the editor's external-agent setting at:
        </p>
        <Code>{`cachycli agent stdio`}</Code>
        <Bullets
          items={[
            "Works with editors that support the agent standard — Zed, Neovim (CodeCompanion, avante.nvim) and Emacs (agent-shell).",
            "Sessions are the same ones you have in the terminal: start in one, continue in the other.",
            "Approvals appear in the editor, using the same four answers.",
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Use your editor's agent in the terminal">
        <Code>{`$ cachycli --backend acp --acp-command "opencode acp"

# or permanently
[agent]
backend = "acp"

[agent.acp]
command = "opencode"
args = ["acp"]`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          You get the same interface as always — thoughts, tool calls, plans and approvals — with
          the agent your editor is already configured for.
        </p>
      </ArticleBlock>

      <ArticleBlock title="An agent on another machine" kicker="[agent.acp] url">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          The agent does not have to run where you type. Give CachyCLI its socket instead of a
          command and the whole protocol crosses the network — same sessions, same approvals, same
          streamed updates.
        </p>
        <Code>{`[agent]
provider = "acp"

[agent.acp]
url = "ws://build-box:8765"`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          That is the other half of <code className="code">agent serve</code>: run the server where
          the repository, the credentials and the tools are, and drive it from anywhere.{" "}
          <code className="code">/provider</code> shows the url as the connection.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Tips">
        <Bullets
          items={[
            "If the editor shows no output, run the agent command by hand first to see its error.",
            "Keep one session per project and rename it — both the editor and the terminal will show the same name.",
          ]}
        />
        <Callout title="Related">
          More on connecting tools is in <a href="/guide/mcp" className="link-underline" style={{ color: "var(--ink)" }}>MCP servers</a>, and on choosing what runs the model in <a href="/guide/providers" className="link-underline" style={{ color: "var(--ink)" }}>Providers</a>.
        </Callout>
      </ArticleBlock>
      <ArticleBlock title="ACP over a socket" kicker="agent serve">
        <p>
          <code className="code">cachycli agent stdio</code> is the usual way an editor talks to
          CachyCLI. The same ACP server also runs over a WebSocket: start{" "}
          <code className="code">cachycli agent serve --port 8765</code> and a client that opens with
          JSON-RPC (<code className="code">initialize</code>) gets the full protocol on that socket —
          sessions, prompts, permissions and streaming updates. Clients that send a plain{" "}
          <code className="code">{"{\"prompt\": \"…\"}"}</code> instead get the simple event
          protocol, so both kinds can share one port.
        </p>
      </ArticleBlock>

    </Article>
  );
}

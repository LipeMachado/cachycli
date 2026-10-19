import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted } from "@/components/ui/prose";

export const metadata: Metadata = {
  title: "MCP servers",
  description: "Connect external tool servers to CachyCLI — and let other agents call CachyCLI.",
};

export default function McpPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 09</Muted>
        <H1 className="mt-4">MCP servers</H1>
        <Lead className="mt-6">
          Tool servers add abilities the agent did not ship with — your issue tracker, your database,
          your notes. Add one and it becomes another tool the agent can use.
        </Lead>
      </header>

      <ArticleBlock title="Add a server" kicker="Two ways">
        <Code label="From the command line">{`$ cachycli mcp add filesystem \\
    --command npx --arg -y --arg @modelcontextprotocol/server-filesystem --arg ~/code

$ cachycli mcp list
$ cachycli mcp test filesystem     # connect and show its tools
$ cachycli mcp remove filesystem`}</Code>
        <Code label="Or in the settings file">{`[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]

[mcp_servers.internal]
url = "https://tools.example.com/mcp"
headers = { authorization = "Bearer \${TOOLS_TOKEN}" }`}</Code>
        <Bullets
          items={[
            "Local servers run as a command; remote servers are used over HTTP.",
            "A server added in a project's own settings only applies to that project.",
            "Environment values can be referenced instead of pasted in.",
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Using the tools">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Connected tools show up alongside the built-in ones. Names are prefixed with the server so
          you always know where they come from, for example{" "}
          <code className="code">filesystem__read_file</code>.
        </p>
        <Code label="See what is connected">{`/mcp`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Let other agents use CachyCLI" kicker="The other direction">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          CachyCLI can also act as a tool server itself, so another agent — opencode included — can
          hand it work.
        </p>
        <Code>{`$ cachycli mcp serve

# register it in opencode
$ opencode mcp add cachy --command "cachycli mcp serve"`}</Code>
        <Bullets
          items={[
            "Ask CachyCLI to run a task in a folder and return the result.",
            "List the models and recent sessions it knows about.",
            "Everything it does still follows the same permissions and sessions.",
          ]}
        />
        <Callout title="Good to know">
          A server you add is started on demand; if it fails,{" "}
          <code className="code">cachycli mcp test &lt;name&gt;</code> shows you the exact error.
        </Callout>
      </ArticleBlock>
    </Article>
  );
}

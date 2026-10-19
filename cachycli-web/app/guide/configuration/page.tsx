import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted, Table } from "@/components/ui/prose";

export const metadata: Metadata = {
  title: "Configuration",
  description: "Theme, defaults, timeouts and per-project settings for CachyCLI.",
};

export default function ConfigurationPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 12</Muted>
        <H1 className="mt-4">Configuration</H1>
        <Lead className="mt-6">
          One settings file for your machine, optionally one per project. Everything has a sensible
          default, so you only write what you want to change.
        </Lead>
      </header>

      <ArticleBlock title="The settings file">
        <Code label="~/.cachy/config.toml">{`[ui]
theme = "cachy-night"        # or cachy-day, tokyonight, rosepine-moon, oscura-midnight
show_thinking = true

[models]
default = "my-model"

[toolset.bash]
timeout_secs = 120           # how long a command may run

[tools]
respect_gitignore = true     # never search ignored folders

[session]
auto_compact_threshold_percent = 85`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Per project">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          A file in the project overrides the one in your home folder. Teams use it to share tool
          servers and defaults.
        </p>
        <Code label=".cachy/config.toml inside the repository">{`[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]`}</Code>
        <Callout title="Order of precedence">
          Project settings win over machine settings; flags and environment variables win over both.
        </Callout>
      </ArticleBlock>

      <ArticleBlock title="Common options">
        <Table
          head={["Setting", "Default", "What it changes"]}
          rows={[
            [<code className="code" key="1">ui.theme</code>, "cachy-night", "Colours of the interface"],
            [<code className="code" key="2">tools.respect_gitignore</code>, "true", "Hide ignored folders from search and pickers"],
            [<code className="code" key="3">toolset.bash.timeout_secs</code>, "120", "How long a command may run before it is stopped"],
            [<code className="code" key="4">session.auto_compact_threshold_percent</code>, "85", "When a long chat is summarised automatically"],
            [<code className="code" key="5">subagents.enabled</code>, "true", "Allow side tasks in their own context"],
            [<code className="code" key="6">memory.enabled</code>, "false", "Keep cross-session notes about a project"],
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Environment variables">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Handy in scripts and containers:
        </p>
        <Table
          head={["Variable", "Effect"]}
          rows={[
            [<code className="code" key="1">CACHY_MODEL</code>, "Default model for new sessions"],
            [<code className="code" key="2">CACHY_AGENT_BACKEND</code>, "opencode, native or acp"],
            [<code className="code" key="3">CACHY_THEME</code>, "Theme name"],
            [<code className="code" key="4">CACHY_MEMORY</code>, "1 to keep project notes"],
            [<code className="code" key="5">CACHY_SEARCH_ENDPOINT</code>, "Your own web-search endpoint"],
          ]}
        />
        <Bullets
          items={[
            <>
              Put the file under version control if you like; it holds no secrets unless you add
              some.
            </>,
            <>
              Check what is in effect for a folder with <code className="code">cachycli inspect</code>.
            </>,
          ]}
        />
      </ArticleBlock>
    </Article>
  );
}

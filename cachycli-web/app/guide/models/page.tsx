import type { Metadata } from "next";
import { Article, ArticleBlock, Bullets, Callout, Code, H1, Lead, Muted, Table } from "@/components/ui/prose";

export const metadata: Metadata = {
  title: "Models & keys",
  description: "Pick models, store keys and point CachyCLI at your own endpoint.",
};

export default function ModelsPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 05</Muted>
        <H1 className="mt-4">Models &amp; keys</H1>
        <Lead className="mt-6">
          The native backend can talk to anything that speaks the common chat API — a hosted
          service, a gateway your team runs, or a model on your laptop.
        </Lead>
      </header>

      <ArticleBlock title="See what is available">
        <Code>{`$ cachycli models          # list everything, with a key indicator
$ cachycli doctor          # is the default model reachable?`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          A few common services work immediately once their key is present. Local servers, such as
          Ollama or LM Studio, need no key at all.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Store a key">
        <Code>{`$ cachycli login --api-key sk-…
$ cachycli logout          # remove it again`}</Code>
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Keys are stored in a local file with owner-only permissions. If you prefer environment
          variables, set them instead — a key in the environment always works.
        </p>
      </ArticleBlock>

      <ArticleBlock title="Add your own endpoint">
        <Code label="Settings file">{`[models]
default = "my-model"

[model.my-model]
model = "the-model-id"          # what the service expects
base_url = "https://api.example.com/v1"
name = "My Model"               # what you see in the picker
env_key = "MY_API_KEY"          # or api_key = "…"
context_window = 128000         # optional, helps with long chats`}</Code>
        <Table
          head={["Field", "What it does"]}
          rows={[
            ["model", "The id sent to the service"],
            ["base_url", "Where to send requests"],
            ["name", "How it appears in /model"],
            ["env_key / api_key", "Where the credential comes from"],
            ["context_window", "Used to warn before a chat gets too long"],
          ]}
        />
      </ArticleBlock>

      <ArticleBlock title="Switch models">
        <Code label="Inside the interface">{`/model                  open the picker
/model my-model         use it straight away

# for one run only
$ cachycli -m my-model -p "explain this file"`}</Code>
      </ArticleBlock>

      <ArticleBlock title="Long conversations">
        <Bullets
          items={[
            <>
              <code className="code">/context</code> shows how full the chat is.
            </>,
            <>
              <code className="code">/compact</code> replaces the older part with a short summary so
              you can keep going.
            </>,
            "Automatic compaction can be tuned in settings if it happens too often or too late.",
          ]}
        />
        <Callout title="Running out of room">
          If a session feels slow or vague, compact it first. Summaries keep decisions and file
          paths; they drop the scrollback you no longer need.
        </Callout>
      </ArticleBlock>
    </Article>
  );
}

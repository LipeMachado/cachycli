import type { Metadata } from "next";
import { Article, ArticleBlock, Code, H1, Lead, Muted } from "@/components/ui/prose";
import { FAQ } from "@/lib/content";

export const metadata: Metadata = {
  title: "Troubleshooting",
  description: "Fixes for the usual problems, and answers to common questions.",
};

const PROBLEMS: { title: string; body: string; fix: string }[] = [
  {
    title: "Nothing answers yet",
    body: "No provider is connected, so there is nothing to talk to.",
    fix: `# inside the interface
/provider
# or from a shell
cachycli doctor`,
  },
  {
    title: "It cannot reach a model",
    body: "The reply comes back as an error about the request or a key.",
    fix: `cachycli doctor
# then either store a key
cachycli login --api-key sk-…
# or use the backend that already works
cachycli --backend opencode`,
  },
  {
    title: "opencode does not answer",
    body: "The opencode backend fails or something never returns.",
    fix: `opencode auth list      # is it logged in?
cachycli --backend opencode`,
  },
  {
    title: "The interface looks broken",
    body: "Characters overlap, or the screen does not repaint.",
    fix: `# make the window wider than 80 columns, then try
cachycli --no-alt-screen
cachycli --minimal       # print into your terminal instead`,
  },
  {
    title: "A tool server never starts",
    body: "A server you added shows up but its tools are missing.",
    fix: `cachycli mcp test my-server
# run the command it prints by hand to see the real error`,
  },
  {
    title: "A command was blocked",
    body: "The agent says the action needs approval but nothing asked.",
    fix: `# in scripts, approve explicitly
cachycli -p "…" --always-approve
cachycli -p "…" --allow "Bash(npm*)"`,
  },
  {
    title: "The chat feels slow or forgetful",
    body: "Answers get vague after a long session.",
    fix: `/compact      # summarise the older part
/context      # see how full it is`,
  },
];

export default function TroubleshootingPage() {
  return (
    <Article>
      <header>
        <Muted>Guide · 14</Muted>
        <H1 className="mt-4">Troubleshooting</H1>
        <Lead className="mt-6">
          Six problems, six fixes. When in doubt, start with{" "}
          <code className="code">cachycli doctor</code>.
        </Lead>
      </header>

      <div className="rule-t">
        {PROBLEMS.map((problem) => (
          <details key={problem.title} className="rule-b group py-5">
            <summary className="flex items-baseline gap-4">
              <span
                className="details-chevron mono-sm"
                style={{ color: "var(--accent)", display: "inline-block" }}
              >
                ▸
              </span>
              <span className="display text-[1.15rem]">{problem.title}</span>
            </summary>
            <div className="mt-4 space-y-4 pl-8">
              <p className="text-[0.95rem]" style={{ color: "var(--ink-60)" }}>
                {problem.body}
              </p>
              <Code>{problem.fix}</Code>
            </div>
          </details>
        ))}
      </div>

      <ArticleBlock title="Questions">
        <div className="rule-t">
          {FAQ.map((item) => (
            <details key={item.q} className="rule-b group py-5">
              <summary className="flex items-baseline gap-4">
                <span
                  className="details-chevron mono-sm"
                  style={{ color: "var(--accent)", display: "inline-block" }}
                >
                  ▸
                </span>
                <span className="display text-[1.15rem]">{item.q}</span>
              </summary>
              <p className="mt-4 pl-8 text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
                {item.a}
              </p>
            </details>
          ))}
        </div>
      </ArticleBlock>

      <ArticleBlock title="Still stuck?">
        <p className="text-[0.96rem]" style={{ color: "var(--ink-60)" }}>
          Open an issue with the command you ran and what you saw. The more exact the command, the
          faster the fix.
        </p>
        <Code>{`$ cachycli version
$ cachycli doctor`}</Code>
      </ArticleBlock>
    </Article>
  );
}

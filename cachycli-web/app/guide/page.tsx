import type { Metadata } from "next";
import Link from "next/link";
import { ButtonLink } from "@/components/ui/button";
import { CardGrid, H1, Lead, Muted } from "@/components/ui/prose";
import { GUIDE_TOPICS, topicHref } from "@/lib/guide";

export const metadata: Metadata = {
  title: "Guide",
  description: "Everything you need to install, connect and use CachyCLI.",
};

export default function GuideIndex() {
  return (
    <article className="max-w-[900px]">
      <Muted>Guide</Muted>
      <H1 className="mt-4">
        Install it.
        <br />
        Connect it. Use it.
      </H1>
      <Lead className="mt-6">
        Short pages, in order. Start with the installation, then pick the backend you already have —
        opencode, your editor, or a model of your own.
      </Lead>

      <div className="mt-8 flex flex-wrap gap-3">
        <ButtonLink href="/guide/installation" size="lg">
          Start with installation
        </ButtonLink>
        <ButtonLink href="/guide/quickstart" variant="ghost" size="lg">
          Jump to quick start
        </ButtonLink>
      </div>

      <div className="mt-12">
        <CardGrid
          columns={3}
          items={GUIDE_TOPICS.filter((topic) => topic.slug).map((topic) => ({
            href: topicHref(topic),
            badge: topic.index,
            title: topic.title,
            body: topic.summary,
          }))}
        />
      </div>

      <p className="mt-10 text-[0.95rem]" style={{ color: "var(--ink-60)" }}>
        Looking for command flags? <Link href="/guide/reference" className="link-underline" style={{ color: "var(--ink)" }}>CLI reference</Link>. Something broken?{" "}
        <Link href="/guide/troubleshooting" className="link-underline" style={{ color: "var(--ink)" }}>Troubleshooting</Link>.
      </p>
    </article>
  );
}

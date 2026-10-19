import type { ReactNode } from "react";

export type GuideTopic = {
  slug: string;
  index: string;
  title: string;
  summary: string;
  /** Short label used by the sidebar. */
  nav?: string;
  badge?: string;
};

/** The guide table of contents: one page per topic, no anchors. */
export const GUIDE_TOPICS: GuideTopic[] = [
  {
    slug: "",
    index: "01",
    title: "Overview",
    summary: "What CachyCLI is, in one screen, and where to start.",
    nav: "Overview",
  },
  {
    slug: "installation",
    index: "02",
    title: "Installation",
    summary: "Install on Linux, macOS or Windows, update it, verify it works.",
  },
  {
    slug: "quickstart",
    index: "03",
    title: "Quick start",
    summary: "Your first session in a project, headless runs and named sessions.",
  },
  {
    slug: "providers",
    index: "04",
    title: "Providers",
    summary: "Connect opencode, Claude Code, your own models or an editor agent.",
  },
  {
    slug: "models",
    index: "05",
    title: "Models & keys",
    summary: "Pick a model, store credentials and point at your own endpoint.",
  },
  {
    slug: "interface",
    index: "06",
    title: "Interface & shortcuts",
    summary: "The prompt box, panels, modals and every keyboard shortcut.",
  },
  {
    slug: "commands",
    index: "07",
    title: "Slash commands",
    summary: "Everything you can type after a slash inside the terminal.",
  },
  {
    slug: "tools",
    index: "08",
    title: "Tools & permissions",
    summary: "What the agent can do and how to approve, allow or block it.",
  },
  {
    slug: "mcp",
    index: "09",
    title: "MCP servers",
    summary: "Connect external tool servers and serve CachyCLI to other agents.",
  },
  {
    slug: "acp",
    index: "10",
    title: "Editors (ACP)",
    summary: "Use CachyCLI inside Zed, Neovim, Emacs and other ACP clients.",
  },
  {
    slug: "sessions",
    index: "11",
    title: "Sessions",
    summary: "Resume, rename, export and inspect what you did before.",
  },
  {
    slug: "configuration",
    index: "12",
    title: "Configuration",
    summary: "Theme, defaults, timeouts and project settings.",
  },
  {
    slug: "reference",
    index: "13",
    title: "CLI reference",
    summary: "Every flag and subcommand with an example.",
  },
  {
    slug: "troubleshooting",
    index: "14",
    title: "Troubleshooting",
    summary: "Common problems and the exact command that fixes them.",
  },
  {
    slug: "agents",
    index: "15",
    title: "Agents & jobs",
    summary: "Profiles, goals with verification, workflows and subagent limits.",
    nav: "Agents & jobs",
  },
];

export function topicHref(topic: GuideTopic) {
  return topic.slug ? `/guide/${topic.slug}` : "/guide";
}

export function topicBySlug(slug: string) {
  return GUIDE_TOPICS.find((t) => t.slug === slug);
}

export function neighbours(slug: string) {
  const i = GUIDE_TOPICS.findIndex((t) => t.slug === slug);
  return {
    previous: i > 0 ? GUIDE_TOPICS[i - 1] : null,
    next: i >= 0 && i < GUIDE_TOPICS.length - 1 ? GUIDE_TOPICS[i + 1] : null,
  };
}

/** Top navigation: three links and one action. */
export const NAV_LINKS: { href: string; label: string; match: string[] }[] = [
  { href: "/", label: "Home", match: ["/"] },
  { href: "/guide", label: "Guide", match: ["/guide"] },
  { href: "https://github.com/LipeMachado/cachycli", label: "GitHub", match: [] },
];

export function isActive(pathname: string, link: { href: string; match: string[] }) {
  if (link.href === "/") return pathname === "/";
  return link.match.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`));
}

/**
 * The single nav entry that owns the current path: the longest matching prefix
 * wins, so "Guide" does not light up on "/guide/reference".
 */
export function activeNavHref(pathname: string): string | null {
  let best: { href: string; length: number } | null = null;
  for (const link of NAV_LINKS) {
    if (!isActive(pathname, link)) continue;
    const length = link.href === "/" ? 1 : link.href.length;
    if (!best || length > best.length) best = { href: link.href, length };
  }
  return best?.href ?? null;
}

export function GuideChildren(_: { children: ReactNode }) {
  return null;
}

# cachycli-web

Marketing site and guide for **CachyCLI** — a coding agent for your terminal.

## Run it

```bash
npm install
npm run dev        # http://localhost:3000
npm run build      # production build (all routes static)
npm start          # serve the production build
```

Deploys to Vercel as-is: every page is prerendered, `not-found.tsx` handles
unknown paths, and `sitemap.xml` / `robots.txt` are generated from the guide
topics.

## Routes

| Route | Content |
| --- | --- |
| `/` | Overview: hero, interface, backends, integrations, install, guide index |
| `/guide` | Guide index — one card per topic |
| `/guide/installation` | Install, update, verify, uninstall |
| `/guide/quickstart` | First session, file references, headless runs, named sessions |
| `/guide/backends` | opencode · your models · your editor |
| `/guide/models` | Models, keys, custom endpoints, context |
| `/guide/interface` | Screen layout, every shortcut, panels, themes |
| `/guide/commands` | Slash commands |
| `/guide/tools` | Tools, approvals, rules, modes |
| `/guide/mcp` | Tool servers, both directions |
| `/guide/acp` | Editors (agent protocol) |
| `/guide/sessions` | Resume, rename, export |
| `/guide/configuration` | Settings file, project overrides, environment |
| `/guide/reference` | Flags, subcommands, output formats |
| `/guide/troubleshooting` | Problems, fixes and questions |
| `not-found` | 404 with links back into the guide |

There are no anchors in the URLs: the guide is one page per topic, with a
sidebar, a mobile topic picker and prev/next pagination.

## Structure

```
app/
  layout.tsx               fonts, nav, footer, smooth scroll
  globals.css              tokens, base styles, custom scrollbars, native controls
  page.tsx                 landing page
  not-found.tsx            404
  sitemap.ts, robots.ts    routes for deployment
  guide/
    layout.tsx             guide shell (sidebar, topic picker, pager)
    page.tsx               guide index
    <topic>/page.tsx       one page per topic
components/
  ui/                      reusable primitives
    button.tsx             Button / ButtonLink — 4 variants, 3 sizes
    section.tsx            Container, Section (tones), SectionHead
    prose.tsx              H1/H2/H3, Lead, Code, Table, Steps, Callout, CardGrid,
                           ShellFrame, KeyTable, Article/ArticleBlock
    reveal.tsx             Reveal (blur in and out), Stagger, Parallax, ScrollProgress
    popover.tsx            accessible popover used by the nav and the guide picker
  site/                    page-level pieces
    nav.tsx                solid → glass bar, logo, install popover, mobile menu
    footer.tsx             slim footer with the interactive wordmark
    wordmark.tsx           CACHYCLI wordmark with a pointer spotlight
    os-install.tsx         system detection, copyable command, download links
    backend-switcher.tsx   opencode / models / editor tabs
    guide-shell.tsx        sidebar, topic picker, prev/next
    marquee.tsx, tick-rail.tsx, smooth-scroll.tsx
lib/
  content.ts               copy used by more than one page
  guide.ts                 guide topics and navigation model
  scroll.ts                Lenis instance + scroll lock
public/shots/*.png         real captures of the shipped program
```

## Version and environment

The version shown in the interface comes from one place:

```bash
# .env.local (see .env.example)
NEXT_PUBLIC_CACHY_VERSION=0.2.0
NEXT_PUBLIC_SITE_URL=https://your-domain
```

## Design language

Industrial minimalism × dark editorial brutalism, in the spirit of the
reference the brief named:

- Ground `#17181c` (soft charcoal, never absolute black), recess `#121317`, one
  bone-paper section `#e7e4dd`, amber accent `#e0af68`.
- Archivo for display, Inter for body, Geist Mono uppercase for labels and
  commands, Instrument Serif italic for statements.
- Square corners, hairline rules, numbered labels, terminal frames around real
  captures, custom scrollbars and styled native controls (details, select,
  selection, focus rings).
- Motion: Lenis smooth scrolling, blur in/out reveals, tab and panel
  transitions, marquee, hover lift. All of it respects
  `prefers-reduced-motion`.

Everything is responsive from 320 px to widescreen; the layouts were checked at
320, 390, 768, 1440 and 1920 px.

## Notes

- Component classes live in `@layer components` so Tailwind utilities can
  override them (for example `hidden md:inline-flex` on a button).
- `allowedDevOrigins` in `next.config.ts` allows loopback and LAN hosts, so
  `npm run dev` hydrates when the page is opened through `127.0.0.1` rather
  than `localhost`.

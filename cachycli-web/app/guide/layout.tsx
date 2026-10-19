import type { Metadata } from "next";
import GuideShell from "@/components/site/guide-shell";

export const metadata: Metadata = {
  title: "Guide",
  description:
    "Install CachyCLI, connect opencode, editors and tool servers, learn the interface, commands and settings.",
};

export default function GuideLayout({ children }: LayoutProps<"/guide">) {
  return <GuideShell>{children}</GuideShell>;
}

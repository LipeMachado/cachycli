import type { Metadata } from "next";
import { Archivo, Geist_Mono, Inter, Instrument_Serif } from "next/font/google";
import "./globals.css";
import Nav from "@/components/site/nav";
import Footer from "@/components/site/footer";
import SmoothScroll from "@/components/site/smooth-scroll";

// Variable fonts: one file each, every weight available (display uses 620).
const archivo = Archivo({ variable: "--font-archivo", subsets: ["latin"] });

const inter = Inter({ variable: "--font-inter", subsets: ["latin"] });

const mono = Geist_Mono({ variable: "--font-mono", subsets: ["latin"] });

// Static family: the display serif is only used in italic statements.
const serif = Instrument_Serif({
  variable: "--font-serif",
  subsets: ["latin"],
  weight: "400",
  style: ["italic"],
});

export const metadata: Metadata = {
  title: {
    default: "Cachy — a coding agent for your terminal",
    template: "%s — Cachy",
  },
  description:
    "Cachy is a terminal coding agent written in Rust: a full-screen interface that drives your opencode install, any ACP agent, or any OpenAI-compatible model — with MCP in both directions.",
  keywords: [
    "terminal",
    "coding agent",
    "opencode",
    "ACP",
    "MCP",
    "Rust",
    "TUI",
    "cachycli",
  ],
  openGraph: {
    title: "Cachy — a coding agent for your terminal",
    description:
      "A Rust TUI coding agent that connects with opencode and other agents.",
    type: "website",
  },
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html
      lang="en"
      className={`${archivo.variable} ${inter.variable} ${mono.variable} ${serif.variable}`}
    >
      <body>
        <SmoothScroll />
        <Nav />
        <main>{children}</main>
        <Footer />
      </body>
    </html>
  );
}

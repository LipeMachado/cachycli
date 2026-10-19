"use client";

import { useEffect, useMemo, useState } from "react";
import { Button, ButtonLink } from "@/components/ui/button";
import { RELEASES, RAW_INSTALL, RAW_INSTALL_PS1, VERSION } from "@/lib/content";

type Platform = "linux" | "macos" | "windows";
type Arch = "x86_64" | "aarch64";

const LABEL: Record<Platform, string> = { linux: "Linux", macos: "macOS", windows: "Windows" };

function detectPlatform(): Platform {
  const ua = navigator.userAgent.toLowerCase();
  const platform = (navigator as unknown as { userAgentData?: { platform?: string } }).userAgentData?.platform?.toLowerCase();
  if (ua.includes("win") || platform?.includes("win")) return "windows";
  if (ua.includes("mac") || platform?.includes("mac")) return "macos";
  return "linux";
}

function detectArch(): Arch {
  const ua = navigator.userAgent.toLowerCase();
  return ua.includes("aarch64") || ua.includes("arm64") ? "aarch64" : "x86_64";
}

/** Install block: detects the system, copies the right command, links the download. */
export default function OsInstall({ compact = false }: { compact?: boolean }) {
  const [platform, setPlatform] = useState<Platform>("linux");
  const [arch, setArch] = useState<Arch>("x86_64");
  const [detected, setDetected] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setPlatform(detectPlatform());
    setArch(detectArch());
    setDetected(true);
  }, []);

  const command = useMemo(
    () => (platform === "windows" ? `irm ${RAW_INSTALL_PS1} | iex` : `curl -fsSL ${RAW_INSTALL} | bash`),
    [platform],
  );

  const asset = useMemo(() => {
    const os = platform === "windows" ? "windows" : platform;
    const ext = platform === "windows" ? "zip" : "tar.gz";
    const normalizedArch = platform === "windows" ? "x86_64" : arch;
    return `cachycli-${VERSION}-${os}-${normalizedArch}.${ext}`;
  }, [platform, arch]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="panel">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-3 border-b px-4 py-3" style={{ borderColor: "var(--line)" }}>
        <div className="flex items-center gap-2" role="tablist" aria-label="Platform">
          {(["linux", "macos", "windows"] as Platform[]).map((p) => {
            const active = p === platform;
            return (
              <button
                key={p}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setPlatform(p)}
                className="mono-sm px-3 py-2 transition-colors duration-300"
                style={{
                  border: "1px solid",
                  borderColor: active ? "var(--ink)" : "var(--line)",
                  background: active ? "var(--ink)" : "transparent",
                  color: active ? "#191a1f" : "var(--ink-60)",
                }}
              >
                {LABEL[p]}
              </button>
            );
          })}
        </div>
        <span className="mono-sm ml-auto" style={{ color: "var(--ink-38)" }}>
          {detected ? `your system: ${LABEL[platform]}` : "checking…"}
        </span>
      </div>

      <div className="p-4 sm:p-5">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-stretch">
          <code
            className="code flex min-h-[48px] min-w-0 flex-1 items-center overflow-x-auto px-4 py-3 max-sm:whitespace-normal max-sm:break-all"
            style={{ background: "var(--bg-deep)", border: "1px solid var(--line)", whiteSpace: "nowrap" }}
          >
            <span style={{ color: "var(--accent)" }}>{platform === "windows" ? "PS> " : "$ "}</span>
            {command}
          </code>
          <Button onClick={copy} size="md" className="sm:min-w-[184px]" aria-live="polite">
            {copied ? "Copied ✓" : "Copy command"}
          </Button>
        </div>

        {!compact && (
          <div className="mt-5 flex flex-wrap gap-3">
            <ButtonLink
              href={platform === "windows" ? RAW_INSTALL_PS1 : RAW_INSTALL}
              external
              variant="ghost"
              size="sm"
            >
              {platform === "windows" ? "Open installer script" : "Open install script"} ↗
            </ButtonLink>
            <ButtonLink href={`${RELEASES}/latest/download/${asset}`} external variant="ghost" size="sm">
              Download for {LABEL[platform]} ↗
            </ButtonLink>
            <ButtonLink href="/guide/installation" variant="link" size="sm">
              Installation guide →
            </ButtonLink>
          </div>
        )}
      </div>
    </div>
  );
}

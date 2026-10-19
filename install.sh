#!/usr/bin/env bash
# CachyCLI installer for Linux and macOS.
#
#   From a checkout:   bash install.sh
#   Remote one-liner:  curl -fsSL https://raw.githubusercontent.com/LipeMachado/cachycli/main/install.sh | bash
#
# Options (environment variables):
#   CACHY_VERSION      install a specific release (e.g. 0.1.0)
#   CACHY_INSTALL_DIR  install directory
#   CACHY_FROM_SOURCE  1 = build with cargo even if a release exists
set -euo pipefail

REPO="${CACHY_UPDATE_REPO:-LipeMachado/cachycli}"
VERSION="${CACHY_VERSION:-}"
# Where the release assets live: GitHub by default, a mirror (or a local
# directory) when rehearsing a release. Assets are <base>/v<version>/<asset>.
RELEASE_BASE="${CACHY_RELEASE_BASE:-https://github.com/${REPO}/releases/download}"
FROM_SOURCE="${CACHY_FROM_SOURCE:-0}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || echo "")"

say() { printf '%s\n' "$*"; }
warn() { printf '! %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"; }

# cargo is often installed in ~/.cargo/bin without being on PATH yet.
find_cargo() {
  if command -v cargo >/dev/null 2>&1; then return 0; fi
  for candidate in "$HOME/.cargo/bin/cargo" /usr/local/bin/cargo /usr/bin/cargo; do
    if [ -x "$candidate" ]; then
      export PATH="$(dirname "$candidate"):$PATH"
      return 0
    fi
  done
  local rustup_cargo
  rustup_cargo="$(ls -1 "$HOME"/.rustup/toolchains/*/bin/cargo 2>/dev/null | head -n1 || true)"
  if [ -n "$rustup_cargo" ]; then
    export PATH="$(dirname "$rustup_cargo"):$PATH"
    return 0
  fi
  return 1
}

# Prefer a directory that is already on PATH so the command works immediately.
pick_install_dir() {
  if [ -n "${CACHY_INSTALL_DIR:-}" ]; then echo "$CACHY_INSTALL_DIR"; return; fi
  case ":$PATH:" in
    *":$HOME/.local/bin:"*) echo "$HOME/.local/bin"; return ;;
  esac
  echo "$HOME/.cachy/bin"
}

INSTALL_DIR="$(pick_install_dir)"

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux) os="linux" ;;
    Darwin) os="macos" ;;
    *) die "unsupported OS: $os" ;;
  esac
  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) die "unsupported architecture: $arch" ;;
  esac
  echo "${os}-${arch}"
}

have_checkout() {
  [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/Cargo.toml" ] && [ -d "$SCRIPT_DIR/crates" ]
}

install_from_source() {
  local src="$1" tmp
  if ! find_cargo; then
    say ""
    say "To build CachyCLI you need the Rust toolchain (one-time, ~1 minute):"
    say "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    say "  source \"\$HOME/.cargo/env\""
    say "Then run this installer again."
    die "cargo not found"
  fi
  local cargo_bin
  cargo_bin="$(command -v cargo)"
  say "Using $cargo_bin"
  if [ -z "$src" ]; then
    need git
    tmp="$(mktemp -d)"
    say "Fetching the sources…"
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmp/cachycli" >/dev/null 2>&1 \
      || die "could not clone https://github.com/${REPO}"
    src="$tmp/cachycli"
  fi
  say "Building with cargo (release)…"
  (cd "$src" && "$cargo_bin" build --release -p cachycli) || die "cargo build failed"
  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$src/target/release/cachycli" "$INSTALL_DIR/cachycli"
}

install_from_release() {
  local target="$1" version="$VERSION"
  need curl
  if [ -z "$version" ]; then
    version="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
      | sed -n 's/.*"tag_name":[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -n1 || true)"
  fi
  [ -n "$version" ] || return 1
  local asset="cachycli-${version}-${target}.tar.gz"
  local url="${RELEASE_BASE}/v${version}/${asset}"
  local tmp
  tmp="$(mktemp -d)"
  say "Downloading ${asset}…"
  curl -fsSL "$url" -o "$tmp/$asset" || { rm -rf "$tmp"; return 1; }
  tar -xzf "$tmp/$asset" -C "$tmp" || { rm -rf "$tmp"; return 1; }
  local bin
  bin="$(find "$tmp" -type f -name cachycli -perm -u+x | head -n1)"
  [ -n "$bin" ] || { rm -rf "$tmp"; return 1; }
  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$bin" "$INSTALL_DIR/cachycli"
  rm -rf "$tmp"
  return 0
}

add_to_path() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return 0 ;;
  esac
  local line="export PATH=\"$INSTALL_DIR:\$PATH\""
  for rc in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
    [ -f "$rc" ] || continue
    if ! grep -q "cachycli installer" "$rc" 2>/dev/null; then
      printf '\n# Added by cachycli installer\n%s\n' "$line" >> "$rc"
      say "Added $INSTALL_DIR to PATH in $rc (restart the shell to pick it up)"
    fi
  done
}

link_extra() {
  # A second, usually-already-on-PATH location for convenience.
  local extra="$HOME/.cachy/bin"
  [ "$INSTALL_DIR" = "$extra" ] && return 0
  case ":$PATH:" in
    *":$extra:"*) ;;&
  esac
  mkdir -p "$extra"
  ln -sf "$INSTALL_DIR/cachycli" "$extra/cachycli" 2>/dev/null || true
}

main() {
  say "CachyCLI installer"
  say "Install directory: $INSTALL_DIR"
  local target installed=0
  target="$(detect_target)"

  if [ "$FROM_SOURCE" = "1" ]; then
    if have_checkout; then install_from_source "$SCRIPT_DIR"; else install_from_source ""; fi
    installed=1
  elif have_checkout; then
    say "Local checkout detected — building it (release assets are optional)."
    install_from_source "$SCRIPT_DIR"
    installed=1
  else
    if install_from_release "$target"; then
      installed=1
    else
      warn "No release available for ${target} yet — building from source instead."
      install_from_source ""
      installed=1
    fi
  fi

  [ "$installed" = "1" ] || die "installation failed"

  add_to_path
  link_extra

  say ""
  if command -v cachycli >/dev/null 2>&1; then
    say "Installed: $(command -v cachycli)"
  else
    say "Installed: $INSTALL_DIR/cachycli"
    say "This shell does not have it on PATH yet — run:"
    say "  export PATH=\"$INSTALL_DIR:\$PATH\""
  fi
  "$INSTALL_DIR/cachycli" version || true
  say ""
  say "Start it with:  cachycli"
  say "First time? Type /provider inside the interface to connect opencode or Claude Code."
}

main "$@"

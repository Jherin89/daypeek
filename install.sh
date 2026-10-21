#!/bin/sh
# daypeek installer: downloads the latest release binary, checks its SHA-256,
# and puts it on your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/Jherin89/daypeek/main/install.sh | sh
#
# Options (after `sh -s --` when piping):
#   --dir DIR     install into DIR (default: ~/.local/bin, or /usr/local/bin if
#                 ~/.local/bin isn't on PATH and /usr/local/bin is writable)
#   --version V   install tag V (e.g. v0.1.0) instead of the latest release
#   --dry-run     show what would happen, change nothing
#
# Supported: macOS on Apple Silicon (arm64), Linux on x86_64. Anything else
# gets the `cargo install` command instead.
set -eu

REPO="Jherin89/daypeek"
BIN="daypeek"
DIR=""
VERSION="latest"
DRY_RUN=0

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B='\033[1;36m' Y='\033[1;33m' R='\033[1;31m' N='\033[0m'
else
  B='' Y='' R='' N=''
fi
say() { printf '%s\n' "$*"; }
info() { printf "${B}==>${N} %s\n" "$*"; }
warn() { printf "${Y}warning:${N} %s\n" "$*" >&2; }
die() { printf "${R}error:${N} %s\n" "$*" >&2; exit 1; }

cargo_hint() {
  say ""
  say "You can build it from source instead (needs Rust 1.85+, see https://rustup.rs):"
  say ""
  say "  cargo install --git https://github.com/$REPO"
  say ""
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dir) [ $# -ge 2 ] || die "--dir needs a value"; DIR="$2"; shift 2 ;;
    --version) [ $# -ge 2 ] || die "--version needs a value"; VERSION="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) sed -n '2,15p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

# ── Which binary? ────────────────────────────────────────────────────────
os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
  Darwin/arm64) target="aarch64-apple-darwin" ;;
  Linux/x86_64|Linux/amd64) target="x86_64-unknown-linux-gnu" ;;
  Darwin/x86_64)
    warn "there's no prebuilt binary for Intel Macs yet."
    cargo_hint; exit 1 ;;
  *)
    warn "there's no prebuilt binary for $os/$arch yet."
    cargo_hint; exit 1 ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL --retry 2 "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q "$1" -O "$2"; }
else
  die "need curl or wget"
fi

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
else
  die "need sha256sum or shasum to verify the download"
fi

# ── Where to put it? ─────────────────────────────────────────────────────
on_path() { case ":$PATH:" in *":$1:"*) return 0 ;; *) return 1 ;; esac; }
if [ -z "$DIR" ]; then
  DIR="$HOME/.local/bin"
  if ! on_path "$DIR" && on_path /usr/local/bin && [ -w /usr/local/bin ]; then
    DIR="/usr/local/bin"
  fi
fi

asset="$BIN-$target.tar.gz"
if [ -n "${DAYPEEK_INSTALL_BASE:-}" ]; then
  base="$DAYPEEK_INSTALL_BASE"   # testing: a local mirror of the release assets
elif [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

info "daypeek for $target ($VERSION release)"
say "    from $base/$asset"
say "    into $DIR/$BIN"

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t daypeek)
trap 'rm -rf "$tmp"' EXIT INT TERM

if ! fetch "$base/$asset" "$tmp/$asset" 2>/dev/null; then
  warn "couldn't download $asset. There may be no published release yet."
  say "    Releases: https://github.com/$REPO/releases"
  cargo_hint
  [ "$DRY_RUN" -eq 1 ] && exit 0
  exit 1
fi
fetch "$base/$BIN-$target.sha256" "$tmp/sum" || die "couldn't download the checksum file"

expected=$(cut -d ' ' -f 1 < "$tmp/sum")
actual=$(sha256 "$tmp/$asset")
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  die "checksum mismatch for $asset (expected $expected, got $actual). Not installing."
fi
info "checksum OK ($actual)"

tar -xzf "$tmp/$asset" -C "$tmp"
[ -f "$tmp/$BIN-$target/$BIN" ] || die "archive didn't contain $BIN"

if [ "$DRY_RUN" -eq 1 ]; then
  info "dry run: would install $DIR/$BIN"
  exit 0
fi

mkdir -p "$DIR" 2>/dev/null || true
if [ -w "$DIR" ]; then
  install -m 755 "$tmp/$BIN-$target/$BIN" "$DIR/$BIN"
else
  info "$DIR isn't writable, using sudo"
  sudo install -m 755 "$tmp/$BIN-$target/$BIN" "$DIR/$BIN"
fi
# Downloaded with curl, so there's normally no quarantine flag; clear it anyway.
if [ "$os" = "Darwin" ] && command -v xattr >/dev/null 2>&1; then
  xattr -d com.apple.quarantine "$DIR/$BIN" 2>/dev/null || true
fi

info "installed $("$DIR/$BIN" --version 2>/dev/null || echo "$BIN") to $DIR/$BIN"
if ! on_path "$DIR"; then
  shell_rc="$HOME/.profile"
  case "${SHELL:-}" in
    */zsh) shell_rc="$HOME/.zshrc" ;;
    */bash) shell_rc="$HOME/.bashrc" ;;
    */fish) shell_rc="$HOME/.config/fish/config.fish" ;;
  esac
  info "$DIR is not on your PATH yet. Add this line to $shell_rc:"
  case "${SHELL:-}" in
    */fish) say "    fish_add_path $DIR" ;;
    *) say "    export PATH=\"$DIR:\$PATH\"" ;;
  esac
fi

say ""
say "Next steps:"
say "  daypeek --demo     # see it with fake data"
say "  daypeek setup      # pick your apps, paste tokens, done"
say "  daypeek            # your day"
say ""
say "Docs: https://github.com/$REPO#readme"

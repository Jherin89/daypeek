#!/bin/sh
# daypeek installer: downloads the latest release binary, checks its SHA-256,
# and puts it on your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/Jherin89/daypeek/main/install.sh | sh
#
# Options (after `sh -s --` when piping):
#   --dir DIR     install into DIR (or set DAYPEEK_INSTALL_DIR=DIR). Default:
#                 the first of ~/.local/bin, ~/bin, /usr/local/bin that is on
#                 your PATH (and writable), else ~/.local/bin plus the exact
#                 line to add it to your PATH
#   --version V   install tag V (e.g. v0.3.0) instead of the latest release
#   --dry-run     show what would happen, change nothing
#
# Prebuilt binaries: macOS (Apple Silicon and Intel) and x86_64 Linux. Anything
# else gets the `cargo install` command instead.
set -eu

REPO="Jherin89/daypeek"
BIN="daypeek"
DIR="${DAYPEEK_INSTALL_DIR:-}"
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
    -h|--help) sed -n '2,16p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

# ── Which binary? ────────────────────────────────────────────────────────
os=$(uname -s)
arch=$(uname -m)
# A Terminal running under Rosetta reports x86_64 on Apple Silicon.
if [ "$os/$arch" = "Darwin/x86_64" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
  arch=arm64
fi
case "$os/$arch" in
  Darwin/arm64|Darwin/aarch64) targets="aarch64-apple-darwin" ;;
  Darwin/x86_64) targets="x86_64-apple-darwin" ;;
  # Static musl build first (any glibc); older releases only have the gnu one.
  Linux/x86_64|Linux/amd64) targets="x86_64-unknown-linux-musl x86_64-unknown-linux-gnu" ;;
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
  for d in "$HOME/.local/bin" "$HOME/bin" /usr/local/bin; do
    if on_path "$d" && { [ -w "$d" ] || { [ ! -e "$d" ] && [ "$d" != /usr/local/bin ]; }; }; then
      DIR="$d"; break
    fi
  done
  [ -n "$DIR" ] || DIR="$HOME/.local/bin"
fi
case "$DIR" in \~/*) DIR="$HOME/${DIR#\~/}" ;; esac

if [ -n "${DAYPEEK_INSTALL_BASE:-}" ]; then
  base="$DAYPEEK_INSTALL_BASE"   # testing: a local mirror of the release assets
elif [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t daypeek)
trap 'rm -rf "$tmp"' EXIT INT TERM

target=""
for t in $targets; do
  if fetch "$base/$BIN-$t.tar.gz" "$tmp/$BIN-$t.tar.gz" 2>/dev/null; then
    target="$t"; break
  fi
done
asset="$BIN-$target.tar.gz"

if [ -z "$target" ]; then
  first=${targets%% *}
  warn "couldn't download $BIN-$first.tar.gz from the $VERSION release."
  if [ "$first" = "x86_64-apple-darwin" ]; then
    say "    Intel Mac binaries ship from v0.2.0 on. For an older release,"
    say "    Homebrew builds it for you:  brew install jherin89/tap/daypeek"
  fi
  say "    Releases: https://github.com/$REPO/releases"
  cargo_hint
  [ "$DRY_RUN" -eq 1 ] && exit 0
  exit 1
fi
info "daypeek for $target ($VERSION release)"
say "    from $base/$asset"
say "    into $DIR/$BIN"
fetch "$base/$BIN-$target.sha256" "$tmp/sum" || die "couldn't download the checksum file"

expected=$(cut -d ' ' -f 1 < "$tmp/sum")
actual=$(sha256 "$tmp/$asset")
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  die "checksum mismatch for $asset (expected $expected, got $actual). Not installing."
fi
info "checksum OK ($actual)"

tar -xzf "$tmp/$asset" -C "$tmp"
[ -f "$tmp/$BIN-$target/$BIN" ] || die "archive didn't contain $BIN"

# Exact PATH advice, printed after a real install and in dry runs.
path_advice() {
  on_path "$DIR" && return 0
  case "${SHELL:-}" in
    */zsh) rc="$HOME/.zshrc" ;;
    */bash) if [ "$os" = "Darwin" ]; then rc="$HOME/.bash_profile"; else rc="$HOME/.bashrc"; fi ;;
    */fish) rc="" ;;
    *) rc="$HOME/.profile" ;;
  esac
  warn "$DIR is not on your PATH, so typing \`daypeek\` won't find it yet."
  say "    Fix it by pasting this one line (then open a new terminal):"
  if [ -z "$rc" ]; then
    say "    fish_add_path $DIR"
  else
    # shellcheck disable=SC2016 # $PATH is meant to stay literal in the rc file
    say "    echo 'export PATH=\"$DIR:\$PATH\"' >> $rc && . $rc"
  fi
}
run_hint="$BIN"
on_path "$DIR" || run_hint="$DIR/$BIN"

if [ "$DRY_RUN" -eq 1 ]; then
  info "dry run: would install $DIR/$BIN"
  path_advice
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

if ! version=$("$DIR/$BIN" --version 2>/dev/null); then
  warn "$DIR/$BIN was installed but won't start on this system."
  if [ "$target" = "x86_64-unknown-linux-gnu" ]; then
    say "    This older release needs glibc 2.39+ (Ubuntu 24.04 or newer). Newer"
    say "    releases ship a static build that runs anywhere; or build from source:"
  fi
  cargo_hint
  exit 1
fi
info "installed $version to $DIR/$BIN"
path_advice

say ""
say "Next steps:"
say "  $run_hint --demo     # see it with fake data"
say "  $run_hint setup      # pick your apps, paste tokens, done"
say "  $run_hint            # your day"
say ""
say "Docs: https://github.com/$REPO#readme"

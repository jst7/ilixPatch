#!/usr/bin/env bash
# ilixPatch bootstrap: detects your OS, checks/installs prerequisites and starts the app.
#
#   ./init.sh             check everything, install what's missing (asks first), run the desktop app
#   ./init.sh --check     only check prerequisites, change nothing
#   ./init.sh --yes       don't ask before installing missing prerequisites
#   ./init.sh --cli       run the CLI against the demo project instead of the app
#   ./init.sh --build     build release installers (target/release/bundle/)
#   ./init.sh --help
#
# Safe to re-run: every step is idempotent.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$ROOT/app"
MIN_RUST="1.85.0"  # edition 2024
MIN_NODE="24.0.0"

MODE="dev"
ASSUME_YES=0
for arg in "$@"; do
  case "$arg" in
    --check) MODE="check" ;;
    --cli) MODE="cli" ;;
    --build) MODE="build" ;;
    -y|--yes) ASSUME_YES=1 ;;
    -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg (see --help)" >&2; exit 2 ;;
  esac
done

# ---------- output helpers ----------
if [[ -t 1 ]]; then
  B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; R=$'\033[31m'; C=$'\033[36m'; N=$'\033[0m'
else
  B=""; G=""; Y=""; R=""; C=""; N=""
fi
step() { printf '\n%s==>%s %s%s%s\n' "$C" "$N" "$B" "$*" "$N"; }
ok()   { printf '  %s✓%s %s\n' "$G" "$N" "$*"; }
warn() { printf '  %s!%s %s\n' "$Y" "$N" "$*"; }
fail() { printf '  %s✗%s %s\n' "$R" "$N" "$*"; }
die()  { printf '\n%sError:%s %s\n' "$R" "$N" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

MISSING=0
confirm() {
  # confirm "question" -> 0 if yes. In --check mode never installs.
  [[ "$MODE" == "check" ]] && return 1
  [[ "$ASSUME_YES" == 1 ]] && return 0
  [[ -t 0 ]] || return 1
  local reply
  read -r -p "  ? $1 [y/N] " reply
  [[ "$reply" =~ ^[YySs]$ ]]
}

# version_ge A B  -> true if A >= B (semver-ish, numeric parts only)
version_ge() {
  local a b i
  IFS=. read -r -a a <<<"${1%%[-+]*}"
  IFS=. read -r -a b <<<"${2%%[-+]*}"
  for i in 0 1 2; do
    local x="${a[i]:-0}" y="${b[i]:-0}"
    x="${x//[^0-9]/}"; y="${y//[^0-9]/}"
    ((10#${x:-0} > 10#${y:-0})) && return 0
    ((10#${x:-0} < 10#${y:-0})) && return 1
  done
  return 0
}

# ---------- 1. detect the system ----------
step "Detecting system"
OS="unknown"; DISTRO=""; PKG=""
case "$(uname -s)" in
  Darwin) OS="macos" ;;
  Linux)
    OS="linux"
    if grep -qiE "microsoft|wsl" /proc/version 2>/dev/null; then OS="wsl"; fi
    if [[ -r /etc/os-release ]]; then
      # shellcheck disable=SC1091
      . /etc/os-release
      DISTRO="${ID:-} ${ID_LIKE:-}"
    fi
    case "$DISTRO" in
      *debian*|*ubuntu*) PKG="apt" ;;
      *fedora*|*rhel*|*centos*) PKG="dnf" ;;
      *arch*|*manjaro*) PKG="pacman" ;;
      *suse*) PKG="zypper" ;;
    esac
    ;;
  MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
esac
ARCH="$(uname -m)"
case "$OS" in
  macos) ok "macOS $(sw_vers -productVersion 2>/dev/null || true) ($ARCH)" ;;
  linux) ok "Linux: ${PRETTY_NAME:-unknown distro} ($ARCH)${PKG:+, package manager: $PKG}" ;;
  wsl)   ok "Windows Subsystem for Linux: ${PRETTY_NAME:-} ($ARCH)"
         warn "WSL needs WSLg (Windows 11) to show the app window. Running natively on Windows is smoother." ;;
  windows) ok "Windows (Git Bash/MSYS) ($ARCH)" ;;
  *) die "Unsupported system: $(uname -s)" ;;
esac
[[ "$OS" == "wsl" ]] && OS="linux"

# ---------- 2. system prerequisites ----------
step "System prerequisites"

if have git; then ok "git $(git --version | awk '{print $3}')"; else
  fail "git not found"; MISSING=1
  case "$OS" in
    macos) warn "Install with: xcode-select --install" ;;
    linux) warn "Install git with your package manager" ;;
    windows) warn "Install from https://git-scm.com/download/win" ;;
  esac
fi

if [[ "$OS" == "macos" ]]; then
  if xcode-select -p >/dev/null 2>&1; then ok "Xcode Command Line Tools"; else
    fail "Xcode Command Line Tools not installed"
    if confirm "Install Xcode Command Line Tools now?"; then
      xcode-select --install || true
      die "Finish the Xcode installer window, then run ./init.sh again."
    fi
    MISSING=1
  fi
fi

linux_deps_install() {
  case "$PKG" in
    apt) sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
           libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config ;;
    dnf) sudo dnf check-update || true
         sudo dnf install -y webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel \
           librsvg2-devel libxdo-devel pkgconf-pkg-config gcc gcc-c++ make ;;
    pacman) sudo pacman -Syu --needed --noconfirm webkit2gtk-4.1 base-devel curl wget file openssl \
           appmenu-gtk-module libappindicator-gtk3 librsvg xdotool pkgconf ;;
    zypper) sudo zypper in -y webkit2gtk3-devel libopenssl-devel curl wget file libappindicator3-1 librsvg-devel \
           pkg-config && sudo zypper in -y -t pattern devel_basis ;;
    *) return 1 ;;
  esac
}

if [[ "$OS" == "linux" ]]; then
  if have pkg-config && pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    ok "WebKitGTK 4.1 $(pkg-config --modversion webkit2gtk-4.1)"
  else
    fail "WebKitGTK 4.1 development files (and other Tauri system libraries) not found"
    if [[ -z "$PKG" ]]; then
      warn "Unknown distro: install the Tauri prerequisites manually: https://tauri.app/start/prerequisites/"
      MISSING=1
    elif confirm "Install Tauri system libraries with $PKG (uses sudo)?"; then
      linux_deps_install && ok "System libraries installed" || { fail "Installation failed"; MISSING=1; }
    else
      MISSING=1
    fi
  fi
fi

if [[ "$OS" == "windows" ]]; then
  warn "Windows also needs the Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 10/11):"
  warn "https://tauri.app/start/prerequisites/#windows"
fi

# ---------- 3. Rust ----------
step "Rust toolchain"
# Prefer rustup's cargo over an older system/Homebrew one.
[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"

install_rustup() {
  if [[ "$OS" == "windows" ]]; then
    warn "Download and run rustup-init.exe from https://rustup.rs, then re-run this script."
    return 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile default
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
}

if have rustc && have cargo; then
  RUST_VER="$(rustc --version | awk '{print $2}')"
  RUSTC_PATH="$(command -v rustc)"
  if version_ge "$RUST_VER" "$MIN_RUST"; then
    ok "rustc $RUST_VER ($RUSTC_PATH)"
  else
    fail "rustc $RUST_VER is too old (need ≥ $MIN_RUST) — found at $RUSTC_PATH"
    if have rustup; then
      if confirm "Update Rust with 'rustup update stable'?"; then rustup update stable && ok "Rust updated"; else MISSING=1; fi
    else
      [[ "$RUSTC_PATH" == *homebrew* || "$RUSTC_PATH" == */Cellar/* ]] &&
        warn "This Rust comes from Homebrew. Recommended: 'brew uninstall rust' and use rustup instead."
      if confirm "Install rustup (official installer from https://sh.rustup.rs)?"; then
        install_rustup && ok "rustup installed: $(rustc --version)" || MISSING=1
        if [[ "$(command -v rustc)" != "$HOME/.cargo/bin/rustc" ]]; then
          warn "Another rustc still comes first in PATH ($(command -v rustc)). Remove it or put ~/.cargo/bin first."
        fi
      else
        MISSING=1
      fi
    fi
  fi
else
  fail "Rust (rustc/cargo) not found"
  if confirm "Install Rust with rustup (official installer from https://sh.rustup.rs)?"; then
    install_rustup && ok "Rust installed: $(rustc --version)" || MISSING=1
  else
    MISSING=1
  fi
fi

# ---------- 4. Node.js ----------
step "Node.js"
# Load nvm if present so its node is picked up.
if ! have node && [[ -s "${NVM_DIR:-$HOME/.nvm}/nvm.sh" ]]; then
  # shellcheck disable=SC1091
  . "${NVM_DIR:-$HOME/.nvm}/nvm.sh"
fi

node_install_hint() {
  case "$OS" in
    macos) echo "brew install node   (or use nvm: https://github.com/nvm-sh/nvm)" ;;
    linux) echo "use nvm: https://github.com/nvm-sh/nvm  (distro packages are often too old)" ;;
    windows) echo "https://nodejs.org (LTS installer) or: winget install OpenJS.NodeJS.LTS" ;;
  esac
}

if have node && have npm; then
  NODE_VER="$(node --version | tr -d v)"
  if version_ge "$NODE_VER" "$MIN_NODE"; then ok "node $NODE_VER, npm $(npm --version)"
  else fail "node $NODE_VER is too old (need ≥ $MIN_NODE). Install a newer one: $(node_install_hint)"; MISSING=1; fi
else
  fail "Node.js/npm not found"
  if [[ "$OS" == "macos" ]] && have brew && confirm "Install Node.js with Homebrew?"; then
    brew install node && ok "node $(node --version)" || MISSING=1
  else
    warn "Install it: $(node_install_hint)"; MISSING=1
  fi
fi

# ---------- summary ----------
if [[ "$MISSING" == 1 ]]; then
  die "Some prerequisites are missing (see above). Fix them and run ./init.sh again."
fi
step "All prerequisites OK"
[[ "$MODE" == "check" ]] && exit 0

# ---------- 5. project dependencies ----------
step "Installing frontend dependencies"
cd "$APP_DIR"
if [[ ! -d node_modules || package-lock.json -nt node_modules/.package-lock.json ]]; then
  npm ci --no-audit --no-fund
else
  ok "node_modules up to date"
fi

# ---------- 6. run ----------
case "$MODE" in
  dev)
    step "Starting ilixPatch (first build compiles Rust dependencies, it can take a few minutes)"
    exec npm run tauri dev
    ;;
  cli)
    step "Running the CLI against the demo project"
    cd "$ROOT"
    exec cargo run -q -p ilixpatch-cli -- run examples/demo-project --env dev
    ;;
  build)
    step "Building release installers"
    npm run tauri build
    ok "Installers are in $ROOT/target/release/bundle/"
    ;;
esac

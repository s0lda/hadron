#!/bin/sh
# Hadron Installer
# https://github.com/s0lda/hadron
# Installs hadron, hadron-gluon, and hadron-forge-mcp

set -e

REPO="s0lda/hadron"
VERSION="${HADRON_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
DRY_RUN=0

print_help() {
    cat <<EOF
Hadron Installer

Usage:
    install.sh [OPTIONS]

Options:
    -h, --help           Show this help message and exit
    -v, --version <ver>  Install a specific version (e.g. 0.31.1 or v0.31.1)
    --latest             Install the latest release (default)
    -b, --prefix <dir>   Install directory (default: ~/.local/bin or \$INSTALL_DIR)
    --dry-run            Display installation plan without downloading or modifying files
EOF
}

# Parse command-line options
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            print_help
            exit 0
            ;;
        -v|--version)
            shift
            if [ $# -eq 0 ]; then
                echo "Error: --version requires a version argument" >&2
                exit 1
            fi
            VERSION="$1"
            ;;
        --latest)
            VERSION="latest"
            ;;
        -b|--prefix)
            shift
            if [ $# -eq 0 ]; then
                echo "Error: --prefix requires a directory argument" >&2
                exit 1
            fi
            INSTALL_DIR="$1"
            ;;
        --dry-run)
            DRY_RUN=1
            ;;
        *)
            echo "Error: Unknown option: $1" >&2
            print_help >&2
            exit 1
            ;;
    esac
    shift
done

# Strip leading 'v' from version if present
case "$VERSION" in
    v*) VERSION="${VERSION#v}" ;;
esac

# Detect OS
OS_UNAME=$(uname -s)
case "$OS_UNAME" in
    Linux*)   OS="linux" ;;
    Darwin*)  OS="darwin" ;;
    *)
        echo "Error: Unsupported operating system: $OS_UNAME" >&2
        echo "Hadron prebuilt binaries currently support Linux and macOS." >&2
        exit 1
        ;;
esac

# Detect Architecture
ARCH_UNAME=$(uname -m)
case "$ARCH_UNAME" in
    x86_64|amd64)
        ARCH="x86_64"
        ;;
    arm64|aarch64)
        ARCH="aarch64"
        ;;
    *)
        echo "Error: Unsupported CPU architecture: $ARCH_UNAME" >&2
        exit 1
        ;;
esac

# Resolve target triple
if [ "$OS" = "linux" ]; then
    if [ "$ARCH" != "x86_64" ]; then
        echo "Error: Linux binaries currently only support x86_64 ($ARCH_UNAME detected)" >&2
        exit 1
    fi
    TARGET="x86_64-unknown-linux-gnu"
elif [ "$OS" = "darwin" ]; then
    if [ "$ARCH" = "aarch64" ]; then
        TARGET="aarch64-apple-darwin"
    else
        TARGET="x86_64-apple-darwin"
    fi
fi

# Resolve version if "latest"
if [ "$VERSION" = "latest" ]; then
    # In dry-run mode, if offline or API unavailable, provide default fallback
    RESOLVED_VERSION=""
    if command -v curl >/dev/null 2>&1; then
        LATEST_URL=$(curl -sIL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null || true)
        case "$LATEST_URL" in
            */tag/v*)
                RESOLVED_VERSION="${LATEST_URL##*/tag/v}"
                ;;
            */tag/*)
                RESOLVED_VERSION="${LATEST_URL##*/tag/}"
                ;;
        esac
    fi
    if [ -z "$RESOLVED_VERSION" ]; then
        RESOLVED_VERSION="0.33.1"
    fi
    VERSION="$RESOLVED_VERSION"
fi

TARBALL="hadron-v${VERSION}-${TARGET}.tar.gz"
DOWNLOAD_URL="${HADRON_DOWNLOAD_URL:-https://github.com/${REPO}/releases/download/v${VERSION}/${TARBALL}}"
CHECKSUMS_URL="${HADRON_CHECKSUMS_URL:-${DOWNLOAD_URL}.sha256}"

if [ "$DRY_RUN" -eq 1 ]; then
    echo "Hadron Installer (Dry Run)"
    echo "Would install Hadron v${VERSION}"
    echo "Target architecture: ${TARGET}"
    echo "Target directory: ${INSTALL_DIR}"
    echo "Download URL: ${DOWNLOAD_URL}"
    echo "Checksums URL: ${CHECKSUMS_URL}"
    exit 0
fi

# Pre-requisite checks
DOWNLOADER=""
if command -v curl >/dev/null 2>&1; then
    DOWNLOADER="curl"
elif command -v wget >/dev/null 2>&1; then
    DOWNLOADER="wget"
else
    echo "Error: Neither curl nor wget is available. Please install curl or wget." >&2
    exit 1
fi

download_file() {
    _url="$1"
    _out="$2"
    if [ "$DOWNLOADER" = "curl" ]; then
        curl -fsSL "$_url" -o "$_out"
    else
        wget -qO "$_out" "$_url"
    fi
}

verify_sha256() {
    _file="$1"
    _expected_hash="$2"
    _computed_hash=""

    if command -v sha256sum >/dev/null 2>&1; then
        _computed_hash=$(sha256sum "$_file" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        _computed_hash=$(shasum -a 256 "$_file" | awk '{print $1}')
    elif command -v openssl >/dev/null 2>&1; then
        _computed_hash=$(openssl dgst -sha256 "$_file" | awk '{print $NF}')
    else
        echo "Warning: No sha256 tool found (sha256sum, shasum, openssl). Skipping checksum verification." >&2
        return 0
    fi

    if [ "$_computed_hash" != "$_expected_hash" ]; then
        echo "Error: Checksum verification failed for $_file" >&2
        echo "Expected: $_expected_hash" >&2
        echo "Got:      $_computed_hash" >&2
        return 1
    fi
    return 0
}

# Create staging directory
TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'hadron-install')
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "==> Downloading Hadron v${VERSION} for ${TARGET}..."
download_file "$DOWNLOAD_URL" "$TMP_DIR/$TARBALL"

echo "==> Downloading SHA256 checksums..."
if download_file "$CHECKSUMS_URL" "$TMP_DIR/SHA256SUMS" 2>/dev/null; then
    EXPECTED_SHA=$(grep "$TARBALL" "$TMP_DIR/SHA256SUMS" | awk '{print $1}' || true)
    if [ -z "$EXPECTED_SHA" ] && [ -z "${HADRON_CHECKSUMS_URL:-}" ]; then
        EXPECTED_SHA=$(awk '/^[a-fA-F0-9]{64}([[:space:]]|$)/ {print $1; exit}' "$TMP_DIR/SHA256SUMS")
    fi
    if [ -n "$EXPECTED_SHA" ]; then
        echo "==> Verifying SHA256 checksum..."
        verify_sha256 "$TMP_DIR/$TARBALL" "$EXPECTED_SHA"
        echo "==> Checksum verified successfully."
    fi
elif [ -z "${HADRON_CHECKSUMS_URL:-}" ] && download_file "https://github.com/${REPO}/releases/download/v${VERSION}/SHA256SUMS" "$TMP_DIR/SHA256SUMS" 2>/dev/null; then
    EXPECTED_SHA=$(grep "$TARBALL" "$TMP_DIR/SHA256SUMS" | awk '{print $1}' || true)
    if [ -n "$EXPECTED_SHA" ]; then
        echo "==> Verifying SHA256 checksum..."
        verify_sha256 "$TMP_DIR/$TARBALL" "$EXPECTED_SHA"
        echo "==> Checksum verified successfully."
    fi
fi

echo "==> Unpacking binaries..."
mkdir -p "$TMP_DIR/extracted"
tar -xzf "$TMP_DIR/$TARBALL" -C "$TMP_DIR/extracted"

BIN_SRC_DIR="$TMP_DIR/extracted"
if [ -d "$TMP_DIR/extracted/hadron-v${VERSION}-${TARGET}" ]; then
    BIN_SRC_DIR="$TMP_DIR/extracted/hadron-v${VERSION}-${TARGET}"
fi

mkdir -p "$INSTALL_DIR"

# Install binaries (unlink destination first to avoid ETXTBSY on running binaries)
for bin_name in hadron hadron-gluon hadron-forge-mcp; do
    if [ -f "$BIN_SRC_DIR/$bin_name" ]; then
        rm -f "$INSTALL_DIR/$bin_name" 2>/dev/null || true
        cp -f "$BIN_SRC_DIR/$bin_name" "$INSTALL_DIR/$bin_name"
        chmod +x "$INSTALL_DIR/$bin_name"
    else
        echo "Warning: Binary $bin_name not found in release archive." >&2
    fi
done

# macOS Gatekeeper handling
if [ "$OS" = "darwin" ] && command -v xattr >/dev/null 2>&1; then
    for bin_name in hadron hadron-gluon hadron-forge-mcp; do
        if [ -f "$INSTALL_DIR/$bin_name" ]; then
            xattr -d com.apple.quarantine "$INSTALL_DIR/$bin_name" 2>/dev/null || true
        fi
    done
fi

# Linux library sanity warnings
if [ "$OS" = "linux" ]; then
    HAS_XKB=0
    HAS_VULKAN=0
    if command -v ldconfig >/dev/null 2>&1; then
        if ldconfig -p 2>/dev/null | grep -q "libxkbcommon"; then
            HAS_XKB=1
        fi
        if ldconfig -p 2>/dev/null | grep -q "libvulkan"; then
            HAS_VULKAN=1
        fi
    fi

    if [ "$HAS_XKB" -eq 0 ]; then
        echo ""
        echo "Notice: libxkbcommon not detected. If Hadron chamber UI does not start, install it via:"
        echo "  sudo apt install libxkbcommon0 libxkbcommon-x11-0    # Debian / Ubuntu"
        echo "  sudo dnf install libxkbcommon                        # Fedora / RHEL"
        echo "  sudo pacman -S libxkbcommon                          # Arch"
    fi

    if [ "$HAS_VULKAN" -eq 0 ]; then
        echo ""
        echo "Notice: Vulkan runtime not detected. If Hadron chamber UI does not start, install it via:"
        echo "  sudo apt install libvulkan1 mesa-vulkan-drivers    # Debian / Ubuntu"
        echo "  sudo dnf install vulkan-loader mesa-vulkan-drivers # Fedora / RHEL"
        echo "  sudo pacman -S vulkan-icd-loader                   # Arch"
    fi
fi

echo ""
echo "========================================================"
echo " Hadron v${VERSION} installed successfully to ${INSTALL_DIR}"
echo "========================================================"
echo ""

# Check PATH
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Notice: $INSTALL_DIR is not in your PATH."
        echo "To add it, append the following line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
        echo ""
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
        ;;
esac

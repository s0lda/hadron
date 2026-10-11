#!/usr/bin/env bash
set -euo pipefail

# 1. Syntax check with sh (POSIX compliance)
sh -n install.sh
echo "install.sh syntax check passed"

# 2. Test install.sh help
./install.sh --help | grep -q "Hadron Installer"
echo "install.sh --help check passed"

# 3. Test argument parsing & dry run
TMP_TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_TEST_DIR"' EXIT

./install.sh --dry-run --prefix "$TMP_TEST_DIR/bin" --version 0.31.1 | grep -q "Would install Hadron v0.31.1"
echo "install.sh dry-run with --version check passed"

./install.sh --dry-run --prefix "$TMP_TEST_DIR/bin" --latest | grep -q "Would install Hadron"
echo "install.sh dry-run with --latest check passed"

./install.sh --dry-run -b "$TMP_TEST_DIR/bin" --version 0.31.1 | grep -q "Target directory: $TMP_TEST_DIR/bin"
echo "install.sh dry-run with -b check passed"

# 4. End-to-end installation test with mock release archive and checksum verification
MOCK_STAGE="$TMP_TEST_DIR/mock_release"
mkdir -p "$MOCK_STAGE"

# Detect host target matching install.sh
OS=$(uname -s)
ARCH=$(uname -m)
if [ "$OS" = "Linux" ]; then
    TARGET="x86_64-unknown-linux-gnu"
elif [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "arm64" ] || [ "$ARCH" = "aarch64" ]; then
        TARGET="aarch64-apple-darwin"
    else
        TARGET="x86_64-apple-darwin"
    fi
fi

TARBALL_NAME="hadron-v0.31.1-${TARGET}.tar.gz"

# Create mock binaries
mkdir -p "$MOCK_STAGE/bin_src"
echo "#!/bin/sh" > "$MOCK_STAGE/bin_src/hadron"
echo 'echo "hadron 0.31.1"' >> "$MOCK_STAGE/bin_src/hadron"
echo "#!/bin/sh" > "$MOCK_STAGE/bin_src/hadron-gluon"
echo 'echo "gluon mock"' >> "$MOCK_STAGE/bin_src/hadron-gluon"
echo "#!/bin/sh" > "$MOCK_STAGE/bin_src/hadron-forge-mcp"
echo 'echo "forge mock"' >> "$MOCK_STAGE/bin_src/hadron-forge-mcp"
chmod +x "$MOCK_STAGE"/bin_src/*

tar -czf "$MOCK_STAGE/$TARBALL_NAME" -C "$MOCK_STAGE/bin_src" hadron hadron-gluon hadron-forge-mcp
(cd "$MOCK_STAGE" && sha256sum "$TARBALL_NAME" > SHA256SUMS)

INSTALL_DEST="$TMP_TEST_DIR/installed_bin"
HADRON_DOWNLOAD_URL="file://$MOCK_STAGE/$TARBALL_NAME" \
HADRON_CHECKSUMS_URL="file://$MOCK_STAGE/SHA256SUMS" \
./install.sh --version 0.31.1 --prefix "$INSTALL_DEST"

# Verify all 3 binaries were installed and are executable
test -x "$INSTALL_DEST/hadron"
test -x "$INSTALL_DEST/hadron-gluon"
test -x "$INSTALL_DEST/hadron-forge-mcp"
"$INSTALL_DEST/hadron" | grep -q "hadron 0.31.1"
echo "install.sh end-to-end install and extraction passed"

# 5. Verify checksum mismatch causes failure
CORRUPT_DEST="$TMP_TEST_DIR/corrupt_bin"
echo "corrupt payload" > "$MOCK_STAGE/corrupt.tar.gz"
if HADRON_DOWNLOAD_URL="file://$MOCK_STAGE/corrupt.tar.gz" \
   HADRON_CHECKSUMS_URL="file://$MOCK_STAGE/SHA256SUMS" \
   ./install.sh --version 0.31.1 --prefix "$CORRUPT_DEST" 2>/dev/null; then
    echo "Expected install.sh to fail on corrupt checksum but it succeeded" >&2
    exit 1
else
    echo "install.sh checksum mismatch rejection passed"
fi

echo "All install.sh tests passed successfully!"

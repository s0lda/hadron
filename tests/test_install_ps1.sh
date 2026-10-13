#!/usr/bin/env bash
# Test suite for Windows install.ps1 using Windows PowerShell from WSL
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Verify powershell.exe is available and responsive
if ! command -v powershell.exe >/dev/null 2>&1; then
    echo "Notice: powershell.exe not found on PATH. Skipping Windows install.ps1 tests on non-WSL environment."
    exit 0
fi

run_ps() {
    powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$@" < /dev/null
}

WIN_SCRIPT_PATH=$(wslpath -w "$REPO_ROOT/install.ps1")

echo "==> 1. Testing install.ps1 -Help"
run_ps "& '$WIN_SCRIPT_PATH' -Help" | grep "Hadron Windows Installer" >/dev/null
echo "install.ps1 -Help check passed"

echo "==> 2. Testing install.ps1 -DryRun"
run_ps "& '$WIN_SCRIPT_PATH' -DryRun -Version 0.31.1" | grep "Would install Hadron v0.31.1" >/dev/null
run_ps "& '$WIN_SCRIPT_PATH' -DryRun -Latest" | grep "Would install Hadron" >/dev/null
run_ps "& '$WIN_SCRIPT_PATH' -DryRun -Version 0.31.1 -DesktopShortcut" | grep "Desktop Shortcut: True" >/dev/null
echo "install.ps1 -DryRun checks passed"

echo "==> 3. Testing end-to-end extraction and SHA256 checksum verification"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

STAGE_DIR="$TMP_DIR/mock_release"
mkdir -p "$STAGE_DIR/bin_src"

# Create mock binaries and icon
echo "echo hadron.exe mock 0.31.1" > "$STAGE_DIR/bin_src/hadron.exe"
echo "echo hadron-gluon.exe mock" > "$STAGE_DIR/bin_src/hadron-gluon.exe"
echo "echo hadron-forge-mcp.exe mock" > "$STAGE_DIR/bin_src/hadron-forge-mcp.exe"
cp "$REPO_ROOT/crates/hadron-chamber/assets/hadron.ico" "$STAGE_DIR/bin_src/hadron.ico"

ARCHIVE_NAME="hadron-v0.31.1-x86_64-pc-windows-msvc.zip"
# Create zip archive using powershell or zip tool
(cd "$STAGE_DIR/bin_src" && zip -q -r "$STAGE_DIR/$ARCHIVE_NAME" hadron.exe hadron-gluon.exe hadron-forge-mcp.exe hadron.ico)
(cd "$STAGE_DIR" && sha256sum "$ARCHIVE_NAME" > SHA256SUMS)

INSTALL_DEST="$TMP_DIR/installed_bin"
mkdir -p "$INSTALL_DEST"

WIN_STAGE_DIR=$(wslpath -w "$STAGE_DIR")
WIN_INSTALL_DEST=$(wslpath -w "$INSTALL_DEST")
WIN_ARCHIVE_URL="file://$WIN_STAGE_DIR\\$ARCHIVE_NAME"
WIN_CHECKSUM_URL="file://$WIN_STAGE_DIR\\SHA256SUMS"

run_ps "
    \$env:HADRON_DOWNLOAD_URL = '$WIN_ARCHIVE_URL'
    \$env:HADRON_CHECKSUMS_URL = '$WIN_CHECKSUM_URL'
    & '$WIN_SCRIPT_PATH' -Version 0.31.1 -InstallDir '$WIN_INSTALL_DEST' -NoShortcut -NoModifyPath
"

# Verify all files exist in installation destination
test -f "$INSTALL_DEST/hadron.exe"
test -f "$INSTALL_DEST/hadron-gluon.exe"
test -f "$INSTALL_DEST/hadron-forge-mcp.exe"
test -f "$INSTALL_DEST/hadron.ico"
echo "install.ps1 end-to-end installation and checksum verification passed"

echo "==> 4. Testing checksum mismatch rejection"
CORRUPT_DEST="$TMP_DIR/corrupt_bin"
mkdir -p "$CORRUPT_DEST"
WIN_CORRUPT_DEST=$(wslpath -w "$CORRUPT_DEST")
echo "corrupted content" > "$STAGE_DIR/corrupt.zip"
WIN_CORRUPT_URL="file://$WIN_STAGE_DIR\\corrupt.zip"

if run_ps "
    \$env:HADRON_DOWNLOAD_URL = '$WIN_CORRUPT_URL'
    \$env:HADRON_CHECKSUMS_URL = '$WIN_CHECKSUM_URL'
    & '$WIN_SCRIPT_PATH' -Version 0.31.1 -InstallDir '$WIN_CORRUPT_DEST' -NoShortcut -NoModifyPath
" 2>/dev/null; then
    echo "Expected install.ps1 to fail on corrupt checksum but it succeeded" >&2
    exit 1
else
    echo "install.ps1 corrupt checksum rejection passed"
fi

echo "==> 5. Testing in-place update while hadron.exe is actively running and locked by Windows"
run_ps "
    \$lockDir = Join-Path \$env:TEMP ('hadron-ps1-lock-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path \$lockDir -Force | Out-Null
    \$pingExe = (Get-Command ping.exe).Source
    \$runningExe = Join-Path \$lockDir 'hadron.exe'
    Copy-Item -Path \$pingExe -Destination \$runningExe -Force
    \$proc = Start-Process -FilePath \$runningExe -ArgumentList '-n','30','127.0.0.1' -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 300
    try {
        \$env:HADRON_DOWNLOAD_URL = '$WIN_ARCHIVE_URL'
        \$env:HADRON_CHECKSUMS_URL = '$WIN_CHECKSUM_URL'
        & '$WIN_SCRIPT_PATH' -Version 0.31.1 -InstallDir \$lockDir -NoShortcut -NoModifyPath
        if (-not (Test-Path (Join-Path \$lockDir 'hadron.exe.old'))) {
            throw 'Expected running hadron.exe to be renamed to hadron.exe.old'
        }
        \$newContent = Get-Content -Path (Join-Path \$lockDir 'hadron.exe') -Raw
        if (\$newContent -notmatch 'hadron.exe mock 0.31.1') {
            throw 'Expected new hadron.exe binary to be written at destination'
        }
    } finally {
        Stop-Process -Id \$proc.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200
        Remove-Item -Recurse -Force \$lockDir -ErrorAction SilentlyContinue
    }
"
echo "install.ps1 in-place running executable replacement passed"

echo "All install.ps1 tests passed successfully!"

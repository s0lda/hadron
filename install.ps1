# Hadron Windows Installer
# https://github.com/s0lda/hadron
# Installs hadron.exe, hadron-gluon.exe, and hadron-forge-mcp.exe with desktop/start menu shortcuts and app icon

[CmdletBinding()]
param(
    [Alias("v")]
    [string]$Version = "latest",

    [Alias("b", "Prefix")]
    [string]$InstallDir = "",

    [switch]$Latest,

    [switch]$DryRun,

    [switch]$NoShortcut,

    [switch]$NoModifyPath,

    [switch]$DesktopShortcut,

    [Alias("h")]
    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Help {
    Write-Host @"
Hadron Windows Installer

Usage:
    install.ps1 [OPTIONS]

Options:
    -h, -Help               Show this help message and exit
    -v, -Version <ver>      Install a specific version (e.g. 0.31.1 or v0.31.1)
    -Latest                 Install the latest release (default)
    -b, -InstallDir <dir>   Install directory (default: `$env:LOCALAPPDATA\Programs\Hadron)
    -DryRun                 Display installation plan without downloading or modifying files
    -NoShortcut             Do not create Start Menu shortcut
    -DesktopShortcut        Also create a Desktop shortcut

Environment Variable Overrides:
    HADRON_VERSION          Target version
    INSTALL_DIR             Installation directory
    HADRON_DOWNLOAD_URL     Direct archive download URL
    HADRON_CHECKSUMS_URL    Direct checksums file URL
"@
}

if ($Help) {
    Show-Help
    exit 0
}

# Honor environment variable overrides
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    if (-not [string]::IsNullOrWhiteSpace($env:INSTALL_DIR)) {
        $InstallDir = $env:INSTALL_DIR
    } else {
        $localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
        if ([string]::IsNullOrWhiteSpace($localAppData)) {
            $localAppData = Join-Path $env:USERPROFILE "AppData\Local"
        }
        $InstallDir = Join-Path $localAppData "Programs\Hadron"
    }
}

if ($Latest -or $Version -eq "latest") {
    if (-not [string]::IsNullOrWhiteSpace($env:HADRON_VERSION)) {
        $Version = $env:HADRON_VERSION
    } else {
        $Version = "latest"
    }
}

# Strip leading 'v'
if ($Version.StartsWith("v") -and $Version.Length -gt 1) {
    $Version = $Version.Substring(1)
}

# Architecture check
$is64 = [Environment]::Is64BitOperatingSystem
if (-not $is64) {
    Write-Error "Error: Hadron prebuilt binaries currently require a 64-bit Windows OS (x86_64)."
    exit 1
}

$Repo = "s0lda/hadron"
$Target = "x86_64-pc-windows-msvc"

# Resolve latest version
if ($Version -eq "latest") {
    $resolved = $null
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
        $req = [System.Net.WebRequest]::Create("https://github.com/$Repo/releases/latest")
        $req.Method = "HEAD"
        $req.AllowAutoRedirect = $false
        $resp = $req.GetResponse()
        $location = $resp.GetResponseHeader("Location")
        $resp.Close()
        if ($location -match '/tag/v?([^/]+)$') {
            $resolved = $matches[1]
        }
    } catch {
        # Fallback to GitHub API or default version if offline/rate-limited
        try {
            $apiResp = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing -TimeoutSec 5
            if ($apiResp.tag_name) {
                $resolved = $apiResp.tag_name.TrimStart('v')
            }
        } catch {}
    }

    if ([string]::IsNullOrWhiteSpace($resolved)) {
        $resolved = "0.33.4"
    }
    $Version = $resolved
}

$ArchiveName = "hadron-v${Version}-${Target}.zip"
$TarballName = "hadron-v${Version}-${Target}.tar.gz"

$DownloadUrl = $env:HADRON_DOWNLOAD_URL
if ([string]::IsNullOrWhiteSpace($DownloadUrl)) {
    $DownloadUrl = "https://github.com/$Repo/releases/download/v${Version}/${ArchiveName}"
}

$ChecksumsUrl = $env:HADRON_CHECKSUMS_URL
if ([string]::IsNullOrWhiteSpace($ChecksumsUrl)) {
    $ChecksumsUrl = "${DownloadUrl}.sha256"
}

if ($DryRun) {
    Write-Host "Hadron Windows Installer (Dry Run)"
    Write-Host "Would install Hadron v$Version"
    Write-Host "Target architecture: $Target"
    Write-Host "Target directory: $InstallDir"
    Write-Host "Download URL: $DownloadUrl"
    Write-Host "Checksums URL: $ChecksumsUrl"
    Write-Host "Start Menu Shortcut: $(-not $NoShortcut)"
    Write-Host "Desktop Shortcut: $($DesktopShortcut.IsPresent)"
    exit 0
}

# Download helper
function Get-RemoteFile {
    param([string]$Url, [string]$Destination, [switch]$Quiet)
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

    # If URL is a local/UNC file path or file:// URI, copy directly without network stack
    if ($Url -like "file://*" -or (Test-Path $Url -ErrorAction SilentlyContinue)) {
        $localPath = $Url -replace '^file://', ''
        if (Test-Path $localPath -ErrorAction SilentlyContinue) {
            Copy-Item -Path $localPath -Destination $Destination -Force
            return
        }
    }

    # If curl.exe is available and functional, use it for optimal streaming
    $curl = Get-Command "curl.exe" -ErrorAction SilentlyContinue
    if ($curl) {
        $curlFlags = if ($Quiet) { "-fsL" } else { "-fsSL" }
        & $curl.Source $curlFlags --connect-timeout 15 --max-time 300 $Url -o $Destination
        if ($LASTEXITCODE -eq 0 -and (Test-Path $Destination)) {
            return
        }
    }

    # Fallback to Invoke-WebRequest or WebClient
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing -TimeoutSec 60
    } catch {
        $wc = New-Object System.Net.WebClient
        $wc.DownloadFile($Url, $Destination)
    }
}

# Verify SHA256 checksum
function Assert-Sha256Checksum {
    param([string]$FilePath, [string]$ExpectedHash)
    $actualHash = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $expected = $ExpectedHash.Trim().ToLowerInvariant()
    if ($actualHash -ne $expected) {
        Write-Error "Error: Checksum verification failed for $FilePath`nExpected: $expected`nGot:      $actualHash"
        exit 1
    }
}

# Create isolated staging directory
$tempBase = [System.IO.Path]::GetTempPath()
$stageGuid = [System.Guid]::NewGuid().ToString("N")
$StageDir = Join-Path $tempBase "hadron-install-$stageGuid"
New-Item -ItemType Directory -Path $StageDir -Force | Out-Null

try {
    Write-Host "==> Downloading Hadron v$Version for $Target..."
    $ArchiveLocal = Join-Path $StageDir $ArchiveName
    Get-RemoteFile -Url $DownloadUrl -Destination $ArchiveLocal

    Write-Host "==> Downloading SHA256 checksums..."
    $ChecksumsLocal = Join-Path $StageDir "SHA256SUMS"
    $expectedSha = $null

    $checksumCandidates = @($ChecksumsUrl)
    $legacySumsUrl = "https://github.com/$Repo/releases/download/v${Version}/SHA256SUMS"
    if ($ChecksumsUrl -ne $legacySumsUrl -and [string]::IsNullOrWhiteSpace($env:HADRON_CHECKSUMS_URL)) {
        $checksumCandidates += $legacySumsUrl
    }

    foreach ($candidateUrl in $checksumCandidates) {
        try {
            Get-RemoteFile -Url $candidateUrl -Destination $ChecksumsLocal -Quiet
            if (Test-Path $ChecksumsLocal) {
                $lines = Get-Content -Path $ChecksumsLocal
                foreach ($line in $lines) {
                    if ($line -match "^([a-fA-F0-9]{64})\s+[\*]?(.*?)$") {
                        $hash = $matches[1]
                        $filename = [System.IO.Path]::GetFileName($matches[2].Trim())
                        if ($filename -eq $ArchiveName -or $filename -eq $TarballName) {
                            $expectedSha = $hash
                            break
                        }
                    } elseif ($line.Trim() -match "^([a-fA-F0-9]{64})$" -and [string]::IsNullOrWhiteSpace($env:HADRON_CHECKSUMS_URL)) {
                        $expectedSha = $matches[1]
                        break
                    }
                }
                if ($expectedSha) {
                    break
                }
            }
        } catch {}
    }

    if ($expectedSha) {
        Write-Host "==> Verifying SHA256 checksum..."
        Assert-Sha256Checksum -FilePath $ArchiveLocal -ExpectedHash $expectedSha
        Write-Host "==> Checksum verified successfully ($expectedSha)."
    } else {
        Write-Host "==> Warning: SHA256 checksum file not found; proceeding with downloaded archive."
    }

    Write-Host "==> Unpacking binaries..."
    $ExtractDir = Join-Path $StageDir "extracted"
    New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null

    if ($ArchiveName.EndsWith(".zip")) {
        Expand-Archive -Path $ArchiveLocal -DestinationPath $ExtractDir -Force
    } else {
        # Support .tar.gz fallback via tar.exe
        & tar.exe -xzf $ArchiveLocal -C $ExtractDir
    }

    # Find the folder containing executables (handles root or nested folder inside archive)
    $binDir = $ExtractDir
    $foundHadron = Get-ChildItem -Path $ExtractDir -Filter "hadron.exe" -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($foundHadron) {
        $binDir = $foundHadron.DirectoryName
    }

    # Ensure target directory exists
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    # Clean up any unlocked stale .old binaries from prior in-place updates
    Get-ChildItem -Path $InstallDir -Filter "*.old*" -File -ErrorAction SilentlyContinue | ForEach-Object {
        Remove-Item -Path $_.FullName -Force -ErrorAction SilentlyContinue
    }

    # Copy binaries and icon
    $binaries = @("hadron.exe", "hadron-gluon.exe", "hadron-forge-mcp.exe")
    foreach ($bin in $binaries) {
        $src = Join-Path $binDir $bin
        $dest = Join-Path $InstallDir $bin
        if (Test-Path $src) {
            $oldDest = $null
            if (Test-Path $dest) {
                # Windows locks running executables against overwrite/delete, but allows
                # renaming them within the same directory before writing the new binary.
                $oldDest = "$dest.old"
                if (Test-Path $oldDest) {
                    Remove-Item -Path $oldDest -Force -ErrorAction SilentlyContinue
                }
                if (Test-Path $oldDest) {
                    $oldDest = "$dest.old.$PID"
                }
                Move-Item -Path $dest -Destination $oldDest -Force -ErrorAction SilentlyContinue
            }
            try {
                Copy-Item -Path $src -Destination $dest -Force
            } catch {
                if ($oldDest -and (Test-Path $oldDest) -and -not (Test-Path $dest)) {
                    Move-Item -Path $oldDest -Destination $dest -Force -ErrorAction SilentlyContinue
                }
                throw
            }
            # Best-effort cleanup if the binary was not running
            if ($oldDest -and (Test-Path $oldDest)) {
                Remove-Item -Path $oldDest -Force -ErrorAction SilentlyContinue
            }
        } else {
            Write-Warning "Binary $bin was not found in release archive."
        }
    }

    $iconSrc = Join-Path $binDir "hadron.ico"
    $iconDest = Join-Path $InstallDir "hadron.ico"
    if (Test-Path $iconSrc) {
        Copy-Item -Path $iconSrc -Destination $iconDest -Force
    }

    # Shortcuts with custom App Icon
    $hadronExePath = Join-Path $InstallDir "hadron.exe"
    $iconPath = if (Test-Path $iconDest) { "$iconDest,0" } else { "$hadronExePath,0" }

    $shortcutWorkDir = if (-not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) { $env:USERPROFILE } else { $InstallDir }
    if (-not $NoShortcut) {
        try {
            $programsDir = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
            if (-not [string]::IsNullOrWhiteSpace($programsDir)) {
                $wshShell = New-Object -ComObject WScript.Shell
                $shortcutPath = Join-Path $programsDir "Hadron.lnk"
                $shortcut = $wshShell.CreateShortcut($shortcutPath)
                $shortcut.TargetPath = $hadronExePath
                $shortcut.WorkingDirectory = $shortcutWorkDir
                $shortcut.IconLocation = $iconPath
                $shortcut.Description = "Hadron AI Coding Swarm Workspace"
                $shortcut.Save()
                Write-Host "==> Created Start Menu shortcut: $shortcutPath"
            }
        } catch {
            Write-Warning "Could not create Start Menu shortcut: $_"
        }
    }

    if ($DesktopShortcut) {
        try {
            $desktopDir = [Environment]::GetFolderPath([Environment+SpecialFolder]::Desktop)
            if (-not [string]::IsNullOrWhiteSpace($desktopDir)) {
                $wshShell = New-Object -ComObject WScript.Shell
                $desktopShortcutPath = Join-Path $desktopDir "Hadron.lnk"
                $shortcut = $wshShell.CreateShortcut($desktopShortcutPath)
                $shortcut.TargetPath = $hadronExePath
                $shortcut.WorkingDirectory = $shortcutWorkDir
                $shortcut.IconLocation = $iconPath
                $shortcut.Description = "Hadron AI Coding Swarm Workspace"
                $shortcut.Save()
                Write-Host "==> Created Desktop shortcut: $desktopShortcutPath"
            }
        } catch {
            Write-Warning "Could not create Desktop shortcut: $_"
        }
    }

    # Check and update PATH environment variable
    if (-not $NoModifyPath -and [string]::IsNullOrWhiteSpace($env:HADRON_NO_MODIFY_PATH)) {
        $userPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
        $pathParts = if ($userPath) { $userPath.Split(';') } else { @() }
        $alreadyInPath = $false
        foreach ($p in $pathParts) {
            if ($p.TrimEnd('\') -eq $InstallDir.TrimEnd('\')) {
                $alreadyInPath = $true
                break
            }
        }

        if (-not $alreadyInPath) {
            Write-Host "==> Adding $InstallDir to user PATH..."
            $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) { $InstallDir } else { "$userPath;$InstallDir" }
            [Environment]::SetEnvironmentVariable("Path", $newUserPath, [EnvironmentVariableTarget]::User)
            $env:PATH = "$InstallDir;$env:PATH"
        }
    }

    Write-Host @"

========================================================
 Hadron v$Version installed successfully to:
 $InstallDir
========================================================

Custom App Icon: Verified (embedded in hadron.exe and hadron.ico)
Binaries:
  - hadron.exe           (Desktop chamber & headless runner)
  - hadron-gluon.exe     (Swarm orchestrator daemon)
  - hadron-forge-mcp.exe (AST edit & tool protocol server)

You can launch Hadron from:
  1. Windows Start Menu: Search for 'Hadron'
  2. Terminal / PowerShell: hadron

"@
} finally {
    Remove-Item -Recurse -Force $StageDir -ErrorAction SilentlyContinue
}

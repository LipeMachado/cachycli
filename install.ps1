# cachycli installer for Windows (PowerShell).
#
#   irm https://raw.githubusercontent.com/LipeMachado/cachycli/main/install.ps1 | iex
#
# Options:
#   $env:CACHY_VERSION      install a specific version
#   $env:CACHY_INSTALL_DIR  install directory (default: $HOME\.cachy\bin)
#   $env:CACHY_FROM_SOURCE  "1" to build with cargo instead of downloading

$ErrorActionPreference = "Stop"

$Repo = if ($env:CACHY_UPDATE_REPO) { $env:CACHY_UPDATE_REPO } else { "LipeMachado/cachycli" }
$ReleaseBase = if ($env:CACHY_RELEASE_BASE) { $env:CACHY_RELEASE_BASE } else { "https://github.com/$Repo/releases/download" }
$InstallDir = if ($env:CACHY_INSTALL_DIR) { $env:CACHY_INSTALL_DIR } else { Join-Path $HOME ".cachy\bin" }
$Version = $env:CACHY_VERSION
$FromSource = $env:CACHY_FROM_SOURCE -eq "1"

function Say($msg) { Write-Host $msg }

function Get-Target {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        "AMD64" { return "windows-x86_64" }
        "ARM64" { return "windows-aarch64" }
        default { throw "unsupported architecture: $arch" }
    }
}

function Install-FromSource {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo is required for CACHY_FROM_SOURCE=1"
    }
    Say "Building cachycli from source with cargo..."
    if (Test-Path (Join-Path $PSScriptRoot "crates")) {
        cargo build --release -p cachycli --manifest-path (Join-Path $PSScriptRoot "Cargo.toml")
    } else {
        $tmp = Join-Path $env:TEMP ("cachy-src-" + [guid]::NewGuid())
        git clone --depth 1 "https://github.com/$Repo.git" $tmp
        cargo build --release -p cachycli --manifest-path (Join-Path $tmp "Cargo.toml")
        $script:SourceDir = $tmp
    }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item (Join-Path (Split-Path $PSScriptRoot -Parent) "target\release\cachycli.exe") (Join-Path $InstallDir "cachycli.exe") -Force
}

function Install-FromRelease {
    $target = Get-Target
    $version = $Version
    if (-not $version) {
        Say "Fetching latest release..."
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $version = $release.tag_name.TrimStart("v")
    }
    $asset = "cachycli-$version-$target.zip"
    $url = "$ReleaseBase/v$version/$asset"
    $tmp = Join-Path $env:TEMP ("cachy-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    Say "Downloading cachycli $version ($target)..."
    try {
        Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $asset)
    } catch {
        throw "download failed: $url (set CACHY_FROM_SOURCE=1 to build locally)"
    }
    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
    $bin = Get-ChildItem -Path $tmp -Recurse -Filter "cachycli.exe" | Select-Object -First 1
    if (-not $bin) { throw "archive did not contain cachycli.exe" }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $bin.FullName (Join-Path $InstallDir "cachycli.exe") -Force
}

function Add-ToPath {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
        Say "Added $InstallDir to the user PATH."
    }
}

Say "cachycli installer"
if ($FromSource) {
    Install-FromSource
} else {
    Install-FromRelease
}
Add-ToPath
Say ""
Say "Installed to $InstallDir\cachycli.exe"
& (Join-Path $InstallDir "cachycli.exe") version
Say ""
Say "Restart your terminal, then run 'cachycli' to get started."
Say "Tip: connect opencode with: cachycli --backend opencode"

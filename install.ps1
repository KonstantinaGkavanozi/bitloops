# One-command installer for the Cycloops research build of Bitloops.
#
#   irm https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1 | iex
#
# Or, to pass options, download first:
#   irm https://raw.githubusercontent.com/.../install.ps1 -OutFile install.ps1
#   .\install.ps1 -Version v0.0.31-archiver.1 -ExportDir D:\bitloops-archive

[CmdletBinding()]
param(
    [string] $Version    = 'latest',
    [string] $InstallDir = "$env:USERPROFILE\.cycloops\bin",
    [string] $ExportDir  = '',
    [switch] $NoPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo    = 'KonstantinaGkavanozi/bitloops'
$BinName = 'cycloops'

function Info($m) { Write-Host "  $m" }
function Warn($m) { Write-Host "  ! $m" -ForegroundColor Yellow }
function Die($m)  { Write-Host "`nError: $m" -ForegroundColor Red; exit 1 }

# --- 1. platform ------------------------------------------------------------

$arch = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }

switch ($arch) {
    'AMD64' { $target = 'x86_64-pc-windows-msvc' }
    'ARM64' { $target = 'aarch64-pc-windows-msvc' }
    default { Die "Unsupported architecture: $arch" }
}

# --- 2. resolve the tag -----------------------------------------------------

if ($Version -eq 'latest') {
    try {
        # Follow the /releases/latest redirect instead of the API, to avoid
        # the unauthenticated API rate limit.
        $resp = Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest" `
                                  -MaximumRedirection 5 -UseBasicParsing
        $tag = ($resp.BaseResponse.ResponseUri.AbsoluteUri -split '/')[-1]
    } catch {
        Die "Could not reach GitHub to resolve the latest release. $($_.Exception.Message)"
    }
    if (-not $tag -or $tag -eq 'releases') {
        Die 'No published release found. Pass -Version with a specific tag.'
    }
} else {
    $tag = $Version
}

$asset = "$BinName-$target.zip"
$base  = "https://github.com/$Repo/releases/download/$tag"

Write-Host ""
Write-Host "Installing $BinName $tag ($target)"
Write-Host ""

# --- 3. download and verify -------------------------------------------------

$tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

try {
    Info "Downloading $asset"
    $zip = Join-Path $tmp $asset
    try {
        Invoke-WebRequest -Uri "$base/$asset" -OutFile $zip -UseBasicParsing
    } catch {
        Die "Download failed. Does $tag have an asset named $asset?"
    }

    $sumFile = Join-Path $tmp 'checksums-sha256.txt'
    $haveSums = $true
    try {
        Invoke-WebRequest -Uri "$base/checksums-sha256.txt" -OutFile $sumFile -UseBasicParsing
    } catch { $haveSums = $false }

    if ($haveSums) {
        Info 'Verifying checksum'
        $expected = $null
        foreach ($line in Get-Content $sumFile) {
            $parts = $line -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) {
                $expected = $parts[0].ToLower(); break
            }
        }
        if (-not $expected) { Die "No checksum listed for $asset." }
        $actual = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) {
            Die "Checksum mismatch for $asset. Expected $expected, got $actual. Aborting."
        }
    } else {
        Warn "checksums-sha256.txt not published for $tag; skipping verification."
    }

    # --- 4. extract and place ---
    Info 'Extracting'
    $unpack = Join-Path $tmp 'unpack'
    Expand-Archive -Path $zip -DestinationPath $unpack -Force

    $exeSrc = Get-ChildItem -Path $unpack -Recurse -Filter "$BinName.exe" | Select-Object -First 1
    if (-not $exeSrc) { Die "Could not find $BinName.exe inside $asset." }
    $dllSrc = Get-ChildItem -Path $unpack -Recurse -Filter 'duckdb.dll' | Select-Object -First 1
    if (-not $dllSrc) { Die "Could not find duckdb.dll inside $asset. The binary will not start without it." }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $exeDest = Join-Path $InstallDir "$BinName.exe"

    if (Test-Path $exeDest) {
        # A running daemon holds a lock on the exe; renaming works where
        # overwriting does not.
        Info "Backing up existing binary to $BinName.exe.bak"
        try {
            Move-Item $exeDest "$exeDest.bak" -Force
        } catch {
            Die "Could not replace $exeDest — a daemon is probably still running. Stop it (``$BinName daemon stop``) and re-run."
        }
    }

    Copy-Item $exeSrc.FullName $exeDest -Force
    Copy-Item $dllSrc.FullName (Join-Path $InstallDir 'duckdb.dll') -Force

    # --- 5. PATH and research defaults ---
    if (-not $NoPath) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($userPath -notlike "*$InstallDir*") {
            [Environment]::SetEnvironmentVariable('Path', "$userPath;$InstallDir", 'User')
            Info "Added $InstallDir to your user PATH"
        }

        # Telemetry is opt-out only; the upstream PostHog key is compiled in,
        # so without this the fork reports usage to Bitloops.
        [Environment]::SetEnvironmentVariable('BITLOOPS_TELEMETRY_OPTOUT', '1', 'User')

        if ($ExportDir) {
            [Environment]::SetEnvironmentVariable('BITLOOPS_CODE_EXPORT_DIR', $ExportDir, 'User')
            Info "Set BITLOOPS_CODE_EXPORT_DIR to $ExportDir"
        }
    }

    Write-Host ""
    Write-Host "Installed to $exeDest"
    & $exeDest --version

    Write-Host @"

Next steps — open a NEW terminal (existing ones will not see the PATH change), then:

  $BinName daemon start      # leave this running in its own terminal
  cd path\to\your\repo
  $BinName init              # tick every agent you use; "Skip for now" for both embedding prompts

Archives are written to `$env:BITLOOPS_CODE_EXPORT_DIR, default %USERPROFILE%\Desktop\bitloops code.
If OneDrive has redirected your Desktop, set BITLOOPS_CODE_EXPORT_DIR explicitly.
Set it for the terminal or app you launch your agent from, not just any shell.

Note: there is no ignore list. A changed .env or key file is archived in
plain text. Keep the export folder somewhere private.
"@
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

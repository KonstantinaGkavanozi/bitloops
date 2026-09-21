# Moved. The installer for this fork lives at the repository root.
#
# This file used to be upstream's installer, pointing at bitloops/bitloops,
# which installed the OFFICIAL Bitloops binary rather than this research
# build. It now forwards to the right script so the old path cannot quietly
# install the wrong thing.

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

Write-Host 'Note: the installer moved to the repository root; forwarding.'

$rootInstaller = Join-Path (Split-Path -Parent $PSScriptRoot) 'install.ps1'

if (Test-Path $rootInstaller) {
    & $rootInstaller @args
    exit $LASTEXITCODE
}

$url = 'https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1'
$tmp = Join-Path $env:TEMP ("cycloops-install-" + [Guid]::NewGuid().ToString() + ".ps1")
try {
    Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
    & $tmp @args
    exit $LASTEXITCODE
} finally {
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
}

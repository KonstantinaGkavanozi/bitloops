@echo off
setlocal

REM Moved. The installer for this fork lives at the repository root.
REM
REM This file used to be upstream's installer, pointing at bitloops/bitloops,
REM which installed the OFFICIAL Bitloops binary rather than this research
REM build. It now forwards to the right script so the old path cannot quietly
REM install the wrong thing.

echo Note: the installer moved to the repository root; forwarding.

if exist "%~dp0..\install.cmd" (
    call "%~dp0..\install.cmd" %*
    endlocal & exit /b %ERRORLEVEL%
)

set "PS=powershell"
where pwsh >nul 2>&1 && set "PS=pwsh"
"%PS%" -NoProfile -ExecutionPolicy Bypass -Command ^
    "$ErrorActionPreference='Stop'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; $f=Join-Path $env:TEMP ('cycloops-install-'+[Guid]::NewGuid().ToString()+'.ps1'); try { Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1' -OutFile $f -UseBasicParsing; & $f %* } finally { Remove-Item $f -Force -ErrorAction SilentlyContinue }"

endlocal & exit /b %ERRORLEVEL%

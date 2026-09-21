@echo off
setlocal

REM Windows CMD installer for the Cycloops research build of Bitloops.
REM
REM   curl -fsSL https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.cmd -o install.cmd && install.cmd && del install.cmd
REM
REM This is a thin wrapper: it hands off to install.ps1, which does the real
REM work. Keeping one implementation means CMD and PowerShell users cannot
REM drift apart, and PowerShell 5.1 ships with every supported version of
REM Windows, so there is nothing extra to install.
REM
REM Arguments are passed straight through, so the PowerShell parameters work
REM here too:
REM
REM   install.cmd -Version v0.0.31-archiver.1
REM   install.cmd -ExportDir D:\bitloops-archive
REM   install.cmd -FullCli

set "INSTALLER_URL=https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1"
set "PS=powershell"

where pwsh >nul 2>&1 && set "PS=pwsh"

REM Prefer install.ps1 sitting next to this file (a cloned repo); otherwise
REM fetch it.
if exist "%~dp0install.ps1" (
    echo Running %~dp0install.ps1
    "%PS%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
) else (
    echo Downloading installer...
    "%PS%" -NoProfile -ExecutionPolicy Bypass -Command ^
        "$ErrorActionPreference='Stop'; [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; $f=Join-Path $env:TEMP ('cycloops-install-'+[Guid]::NewGuid().ToString()+'.ps1'); try { Invoke-WebRequest -Uri '%INSTALLER_URL%' -OutFile $f -UseBasicParsing; & $f %* } finally { Remove-Item $f -Force -ErrorAction SilentlyContinue }"
)

set "EXITCODE=%ERRORLEVEL%"

if not "%EXITCODE%"=="0" (
    echo.
    echo Install failed with exit code %EXITCODE%.
    endlocal & exit /b %EXITCODE%
)

echo.
echo Open a NEW terminal, then:
echo.
echo     cd path\to\your\repo
echo     cycloops init
echo.

endlocal & exit /b 0

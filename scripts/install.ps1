# Moved. The installer for this fork lives at the repository root.
#
# This file used to be upstream's installer, pointing at bitloops/bitloops,
# which installed the OFFICIAL Bitloops binary rather than this research
# build.
#
# It deliberately does NOT download and run anything: an earlier version did,
# and Windows Defender flagged the pattern as Trojan:Win32/ClickFix. Follow
# the instructions it prints instead.

Write-Host ''
Write-Host 'The installer moved to the repository root.'
Write-Host ''
Write-Host 'From a checkout:'
Write-Host '    .\install.ps1'
Write-Host ''
Write-Host 'Otherwise download it first, look at it, then run it:'
Write-Host '    curl.exe -fsSL -o install.ps1 https://raw.githubusercontent.com/KonstantinaGkavanozi/bitloops/main/install.ps1'
Write-Host '    .\install.ps1'
Write-Host ''
Write-Host 'Or take the zip from the Releases page:'
Write-Host '    https://github.com/KonstantinaGkavanozi/bitloops/releases/latest'
Write-Host ''
exit 1

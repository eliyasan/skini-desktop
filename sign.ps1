# Signs skini.exe with the Certum open-source code signing certificate (SimplySign in the cloud).
# Before running: open SimplySign Desktop and log in, so the certificate shows up in Windows.
# Usage:  powershell -ExecutionPolicy Bypass -File sign.ps1 [path-to-exe]
$ErrorActionPreference = 'Stop'
$exe = if ($args[0]) { $args[0] } else { Join-Path $PSScriptRoot 'target\release\skini.exe' }
$signtool = Join-Path $env:USERPROFILE 'tools\signtool\signtool.exe'
if (-not (Test-Path $signtool)) { Write-Error "signtool not found at $signtool" }

# The Certum certificate in the current user's store (SimplySign puts it there while logged in).
$cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert |
  Where-Object { $_.Issuer -match 'Certum' -and $_.NotAfter -gt (Get-Date) } |
  Sort-Object NotAfter -Descending | Select-Object -First 1
if (-not $cert) { Write-Error 'No valid Certum code signing certificate found. Is SimplySign Desktop running and logged in?' }
Write-Host "Signing with: $($cert.Subject) (until $($cert.NotAfter.ToString('yyyy-MM-dd')))"

& $signtool sign /sha1 $cert.Thumbprint /fd sha256 /tr http://time.certum.pl /td sha256 /d 'Skini' /du 'https://www.applepay.skin' $exe
if ($LASTEXITCODE) { Write-Error 'signtool sign failed' }
& $signtool verify /pa /v $exe | Select-String 'Successfully verified|Issued to'
Write-Host "Signed: $exe" -ForegroundColor Green

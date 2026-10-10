# Builds skini.exe with the GNU toolchain (no Visual Studio needed).
# The windows-* crates call dlltool.exe, which rustup bundles but does not put on
# PATH — so we add the toolchain's self-contained bin dir here.
$ErrorActionPreference = 'Stop'

# A full MinGW-w64 (with as.exe/dlltool.exe/gcc.exe) is required, because the
# windows-* crates generate import libraries with dlltool, which itself needs the
# GNU assembler. rustup's bundled mingw lacks `as`, so we use a full MinGW install.
$mingwBin = Join-Path $env:USERPROFILE 'mingw64\bin'
if (-not (Test-Path (Join-Path $mingwBin 'as.exe'))) {
  Write-Error "MinGW-w64 not found at $mingwBin. Download a winlibs UCRT build and extract it so that $mingwBin\as.exe exists."
}
$env:PATH = "$mingwBin;$env:PATH"

Push-Location $PSScriptRoot
try {
  cargo build --release
  $exe = Join-Path $PSScriptRoot 'target\release\skini.exe'
  if (Test-Path $exe) { Write-Host "Built: $exe" -ForegroundColor Green }
} finally {
  Pop-Location
}

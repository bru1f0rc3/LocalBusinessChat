# Builds x86 + x64 release binaries and the single installer dist\NassiRS-Setup-<ver>.exe
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:Path += ";$env:USERPROFILE\.cargo\bin"

foreach ($t in 'i686-pc-windows-msvc', 'x86_64-pc-windows-msvc') {
    Write-Host "== cargo build $t"
    cargo build --release --target $t --target-dir target\dist
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $t" }
}

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found (winget install JRSoftware.InnoSetup)" }

Write-Host "== ISCC"
& $iscc "$PSScriptRoot\nassi-rs.iss"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
Get-ChildItem "$root\dist\*.exe" | Select-Object Name, Length

# Builds the release binaries and the single installer dist\NassiRS-Setup-<ver>.exe
#
#   x86_64-pc-windows-msvc    (stable)              64-bit Windows 10/11
#   x86_64-win7-windows-msvc  (nightly, build-std)  64-bit Windows 7 SP1 / 8 / 8.1
#   i686-win7-windows-msvc    (nightly, build-std)  32-bit Windows 7 SP1 ... 11
#
# The *-win7-* targets are tier 3 in Rust: std is compiled from source, which needs
# the nightly toolchain with rust-src (installed automatically below).
#
#   -Publish                 also copy the installer into the update folder; every running
#   -UpdateDir \\srv\share   Nassi RS finds it there and updates itself (src/update.rs).
#                            Raise "version" in Cargo.toml first, otherwise nobody updates.
param(
    [switch]$Publish,
    [string]$UpdateDir = '\\server\hole\update'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:Path += ";$env:USERPROFILE\.cargo\bin"

Write-Host "== cargo build x86_64-pc-windows-msvc (stable)"
rustup target add x86_64-pc-windows-msvc | Out-Null
cargo build --release --target x86_64-pc-windows-msvc --target-dir target\dist
if ($LASTEXITCODE -ne 0) { throw "cargo build failed for x86_64-pc-windows-msvc" }

Write-Host "== nightly toolchain + rust-src (for Windows 7/8.1 builds)"
rustup toolchain install nightly --profile minimal --component rust-src
if ($LASTEXITCODE -ne 0) { throw "rustup failed to install nightly" }

foreach ($t in 'x86_64-win7-windows-msvc', 'i686-win7-windows-msvc') {
    Write-Host "== cargo +nightly build $t"
    cargo +nightly build --release '-Zbuild-std=std,panic_abort' --target $t --target-dir target\dist
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $t" }
}

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found (winget install JRSoftware.InnoSetup)" }

$ver = (Select-String -Path "$root\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "== ISCC (version $ver)"
& $iscc "/DAppVersion=$ver" "$PSScriptRoot\nassi-rs.iss"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
Get-ChildItem "$root\dist\*.exe" | Select-Object Name, Length

if ($Publish) {
    $setup = "$root\dist\NassiRS-Setup-$ver.exe"
    if (-not (Test-Path $UpdateDir)) { throw "update folder not reachable: $UpdateDir" }
    # copy under a temporary name, then rename: a program checking right now never sees half a file
    $tmp = Join-Path $UpdateDir "NassiRS-Setup-$ver.exe.part"
    Copy-Item $setup $tmp -Force
    Move-Item $tmp (Join-Path $UpdateDir "NassiRS-Setup-$ver.exe") -Force
    Write-Host "== published to $UpdateDir"
    Get-ChildItem (Join-Path $UpdateDir 'NassiRS-Setup-*.exe') | Select-Object Name, Length, LastWriteTime
}

#!/bin/sh
# Builds the Windows 7 SP1 … 11 binaries (x86 + x64) on Linux with mingw-w64 and the
# installer dist/NassiRS-Setup-<ver>.exe with NSIS (installer/nassi-rs.nsi).
#
#   apt install mingw-w64 nsis
#   rustup toolchain install nightly --component rust-src   (or stable + rust-src and RUSTC_BOOTSTRAP=1)
#
# The *-win7-windows-gnu targets are tier 3: std is built from source (-Zbuild-std).
set -e
cd "$(dirname "$0")/.."
TOOLCHAIN=${TOOLCHAIN:-nightly}
for t in x86_64-win7-windows-gnu i686-win7-windows-gnu; do
    echo "== cargo +$TOOLCHAIN build $t"
    cargo "+$TOOLCHAIN" build --release -Zbuild-std=std,panic_abort --target "$t" --target-dir target/dist
done
ver=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n1)
mkdir -p dist
echo "== makensis (version $ver)"
makensis -V2 -DVERSION="$ver" installer/nassi-rs.nsi
ls -la dist/*.exe

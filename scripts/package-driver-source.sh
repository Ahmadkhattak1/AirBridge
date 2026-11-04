#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:?Usage: package-driver-source.sh OUTPUT_ARCHIVE}
kernel=${2:-/opt/airbridge/kernel}
stage=$(mktemp -d /var/cache/airbridge-source.XXXXXX)
trap '[[ "$stage" == /var/cache/airbridge-source.* ]] && rm -rf -- "$stage"' EXIT
git -C "$kernel" archive HEAD | tar -x -C "$stage"
cp "$kernel/.config" "$stage/airbridge-build.config"
cp "$kernel/Module.symvers" "$stage/airbridge-Module.symvers"
git -C "$kernel" rev-parse HEAD > "$stage/airbridge-upstream-commit.txt"
gcc --version > "$stage/airbridge-compiler-version.txt"
mkdir -p "$stage/airbridge-patches"
cp "$root/runtime/rtl8188f-awdl.patch" "$root/experiments/wifi_ble/rtl8188f-cck-experiment.patch" "$stage/airbridge-patches/"
cat > "$stage/AIRBRIDGE-BUILD.md" <<'EOF'
# AirBridge 0.2.0 matching driver source

This is the complete upstream Microsoft WSL2 Linux kernel source used for the
distributed modules. See airbridge-upstream-commit.txt, COPYING, LICENSES,
airbridge-build.config, and airbridge-compiler-version.txt.

Copy airbridge-build.config to .config. On Linux with the standard kernel build
dependencies, run make olddefconfig and make -j4 modules to build the unmodified
modules, including cfg80211, mac80211, libarc4, led-class, tun, and rtl8xxxu.
Preserve the unmodified rtl8xxxu.ko as the stock variant.

For the AWDL variant, apply airbridge-patches/rtl8188f-awdl.patch to a separate
copy of this tree. For the CCK sender variant, apply only
airbridge-patches/rtl8188f-cck-experiment.patch to another separate copy. Rebuild
drivers/net/wireless/realtek/rtl8xxxu in each configured tree. The repository's
scripts/build-receiver-driver.sh documents the isolated-module build and symbol
dependencies used during development. strip --strip-debug removes debug info
from the distributed copies. No driver change is made to the stock variant.

Both patches are GPL-2.0 driver changes. The independently built Rust application
and radio integration sources are in the AirBridge v0.2.0 Git tag.
EOF
tar -C "$stage" --owner=0 --group=0 -c . | xz -T2 -2 > "$output"

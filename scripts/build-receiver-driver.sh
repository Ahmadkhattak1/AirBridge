#!/usr/bin/env bash
# Build the shared rtl8xxxu AWDL driver against an already built WSL kernel.
set -euo pipefail
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
kernel=${1:-/opt/airbridge/kernel}
[[ ! -f /run/airbridge/session.pid ]] || { echo 'Stop AirBridge before installing its driver.' >&2; exit 1; }
test -f "$kernel/Module.symvers"
test -f "$kernel/include/generated/utsrelease.h"
grep -Fq "\"$(uname -r)\"" "$kernel/include/generated/utsrelease.h" || {
    echo 'The kernel build must match the running WSL kernel.' >&2; exit 1;
}
mkdir -p /var/cache/airbridge-driver
build_dir=$(mktemp -d /var/cache/airbridge-driver/build.XXXXXX)
trap '[[ "$build_dir" == /var/cache/airbridge-driver/build.* ]] && rm -rf -- "$build_dir"' EXIT
relative=drivers/net/wireless/realtek/rtl8xxxu
module_dir="$build_dir/$relative"
mkdir -p "$module_dir"
tar -C "$kernel/$relative" --exclude='*.o' --exclude='*.ko' --exclude='*.cmd' \
    --exclude='*.mod*' --exclude=Module.symvers --exclude=modules.order -cf - . |
    tar -C "$module_dir" -xf -
# The development kernel may contain the independent sender experiment.
# Work only on the isolated copy and leave that kernel source untouched.
if grep -q airbridge_cck "$module_dir/core.c"; then
    patch --batch -R -d "$build_dir" -p1 < "$source_root/experiments/wifi_ble/rtl8188f-cck-experiment.patch"
fi
patch --batch -d "$build_dir" -p1 < "$source_root/runtime/rtl8188f-awdl.patch"
python3 "$source_root/scripts/test-driver-descriptors.py" "$module_dir"
symbols=()
for part in drivers/usb/core net/wireless net/mac80211 drivers/leds; do
    [[ ! -f "$kernel/$part/Module.symvers" ]] || symbols+=("$kernel/$part/Module.symvers")
done
make -C "$kernel" M="$module_dir" CONFIG_RTL8XXXU=m \
    "KBUILD_EXTRA_SYMBOLS=${symbols[*]}" -j"${AIRBRIDGE_BUILD_JOBS:-4}" modules
modinfo -p "$module_dir/rtl8xxxu.ko" | grep -q '^airbridge_inject_rate:'
install -Dm644 "$module_dir/rtl8xxxu.ko" /opt/airbridge/modules/rtl8xxxu-awdl.ko
echo AIRBRIDGE_RECEIVER_DRIVER_INSTALLED

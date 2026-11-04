#!/usr/bin/env bash
set -euo pipefail
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
command -v hostapd >/dev/null || { echo 'Install the hostapd package first.' >&2; exit 1; }
[[ ! -f /run/airbridge/session.pid ]] || { echo 'Stop AirBridge before updating its runtime.' >&2; exit 1; }
modinfo -p /opt/airbridge/modules/rtl8xxxu-awdl.ko | grep '^airbridge_cck:' >/dev/null || {
    echo 'Build the shared driver with scripts/build-receiver-driver.sh first.' >&2; exit 1;
}
while IFS=$'\t' read -r usb_id chipset driver firmware rate; do
    [[ "$usb_id" != \#* && -n "$usb_id" ]] || continue
    if [[ ! -f "/opt/airbridge/firmware/$firmware" ]]; then
        bash "$source_root/scripts/fetch-radio-firmware.sh"
        break
    fi
done < "$source_root/runtime/adapters.tsv"
cd "$source_root"
target=${CARGO_TARGET_DIR:-$source_root/target}
cargo build --release -p airbridge-cli -p airbridge-radio --locked
install -Dm755 "$source_root/runtime/airbridge-radio.sh" /opt/airbridge/bin/airbridge-radio
install -Dm644 "$source_root/runtime/adapter-profile.sh" /opt/airbridge/bin/adapter-profile.sh
install -Dm644 "$source_root/runtime/adapters.tsv" /opt/airbridge/adapters.tsv
install -Dm755 "$target/release/airbridge" /opt/airbridge/bin/airbridge
install -Dm755 "$target/release/airbridge-radio" /opt/airbridge/bin/airbridge-radio-node
mkdir -p /opt/airbridge/modules
kernel_module=/opt/airbridge/kernel/drivers/net/wireless/realtek/rtl8xxxu/rtl8xxxu.ko
if [[ ! -f /opt/airbridge/modules/rtl8xxxu.ko && -f "$kernel_module" ]]; then
    if modinfo -p "$kernel_module" | grep -q '^airbridge_cck:'; then
        echo 'Keep the original rtl8xxxu.ko separately before installing the experimental sender driver.' >&2
        exit 1
    fi
    install -Dm644 "$kernel_module" /opt/airbridge/modules/rtl8xxxu.ko
fi
test -f /opt/airbridge/modules/rtl8xxxu.ko
if modinfo -p /opt/airbridge/modules/rtl8xxxu.ko | grep -q '^airbridge_cck:'; then
    echo 'The normal receiver module must not be the experimental sender module.' >&2
    exit 1
fi
echo AIRBRIDGE_RUNTIME_INSTALLED

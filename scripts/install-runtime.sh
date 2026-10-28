#!/usr/bin/env bash
set -euo pipefail
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
command -v hostapd >/dev/null || { echo 'Install the hostapd package first.' >&2; exit 1; }
[[ ! -f /run/airbridge/session.pid ]] || { echo 'Stop AirBridge before updating its runtime.' >&2; exit 1; }
cd "$source_root"
cargo build --release -p airbridge-cli -p airbridge-radio
install -Dm755 "$source_root/runtime/airbridge-radio.sh" /opt/airbridge/bin/airbridge-radio
install -Dm755 "$source_root/target/release/airbridge" /opt/airbridge/bin/airbridge
install -Dm755 "$source_root/target/release/airbridge-radio" /opt/airbridge/bin/airbridge-radio-node
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

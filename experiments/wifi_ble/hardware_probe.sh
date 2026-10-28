#!/usr/bin/env bash
# Isolated experiment. Keeps /opt/airbridge/modules/rtl8xxxu.ko untouched.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
out="$here/../../output/wifi-ble-model"
run_label="${1:-step}-${2:-0}-$(date -u +%Y%m%dT%H%M%SZ)"
test_module=/opt/airbridge/modules/rtl8xxxu-cck-experiment.ko
original=/opt/airbridge/modules/rtl8xxxu.ko
firmware=/sys/module/firmware_class/parameters/path
saved_firmware=$(cat "$firmware")
mkdir -p /run/airbridge "$out"
exec 9>/run/airbridge/session.lock
flock -n 9 || { echo 'Stop the active AirBridge session first.' >&2; exit 1; }
test -f "$test_module"
test -f "$original"
test "$(modinfo -F vermagic "$test_module")" = "$(modinfo -F vermagic "$original")"
test ! -f /run/airbridge/session.pid
loaded=
monitor=
cleanup() {
    trap - EXIT INT TERM
    [[ -z "$monitor" ]] || iw dev abcck0 del 2>/dev/null || true
    if [[ -n "$loaded" ]]; then
        printf N > /sys/module/rtl8xxxu/parameters/airbridge_cck 2>/dev/null || true
        if rmmod rtl8xxxu; then
            insmod "$original"
        else
            echo 'Test module still loaded with CCK experiment disabled; original module file is unchanged.' >&2
        fi
    fi
    printf '%s' "$saved_firmware" > "$firmware"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p /mnt/wsl/airbridge-firmware
cp -a /opt/airbridge/firmware/. /mnt/wsl/airbridge-firmware/
cp /lib/firmware/regulatory.db /mnt/wsl/airbridge-firmware/
[[ ! -f /lib/firmware/regulatory.db.p7s ]] || cp /lib/firmware/regulatory.db.p7s /mnt/wsl/airbridge-firmware/
printf '%s' /mnt/wsl/airbridge-firmware > "$firmware"
modprobe cfg80211
modprobe mac80211
modprobe led-class
if grep -q '^rtl8xxxu ' /proc/modules; then rmmod rtl8xxxu; fi
insmod "$test_module" airbridge_cck=1
loaded=1
radio=wlx00e0253fff2f
for _ in {1..30}; do [[ ! -e "/sys/class/net/$radio" ]] || break; sleep .1; done
test -e "/sys/class/net/$radio"
ip link set "$radio" down
phy=$(basename "$(readlink -f "/sys/class/net/$radio/phy80211")")
iw phy "$phy" interface add abcck0 type monitor
monitor=1
ip link set abcck0 address 00:e0:25:3f:ff:2f
ip link set abcck0 up
iw dev abcck0 set channel 4 HT20
python3 "$here/transmit.py" --interface abcck0 --seconds 20 --style "${1:-step}" --rotation "${2:-0}" | tee "$out/transmit-$run_label.log"
dmesg | tail -n 45 > "$out/driver-$run_label.log"
iw dev abcck0 del
monitor=
printf N > /sys/module/rtl8xxxu/parameters/airbridge_cck
flock -u 9
followup_args=()
[[ "${3:-}" != send-test ]] || followup_args+=(--send-test)
python3 "$here/followup.py" --output "$out/discovery-$run_label.log" "${followup_args[@]}"

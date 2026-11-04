#!/usr/bin/env bash
# Read the exact USB adapter's capabilities, briefly exercise monitor mode,
# then restore its mode and the firmware search path.
set -euo pipefail
runtime=/opt/airbridge
source "$runtime/bin/adapter-profile.sh"
duration=${1:-8}
[[ "$duration" =~ ^[0-9]+$ && "$duration" -le 180 ]] || exit 2
shared=/mnt/wsl/airbridge-probe
original_firmware=$(cat /sys/module/firmware_class/parameters/path)
radio=
cleanup() {
    printf '%s\n' "$original_firmware" > /sys/module/firmware_class/parameters/path
    if [[ -n "$radio" ]]; then
        ip link set "$radio" down || true
        iw dev "$radio" set type managed || true
    fi
}
trap cleanup EXIT
mkdir -p "$shared/rtlwifi"
cp -au "$runtime/firmware/rtlwifi/." "$shared/rtlwifi/"
cp "$runtime/firmware/regulatory.db" "$runtime/firmware/regulatory.db.p7s" "$shared/"
printf '%s' "$shared" > /sys/module/firmware_class/parameters/path
modprobe cfg80211
modprobe mac80211
modprobe led-class
grep -q '^rtl8xxxu ' /proc/modules || insmod "$runtime/modules/rtl8xxxu.ko"
select_adapter "$runtime/adapters.tsv" /sys/bus/usb/devices || { echo 'Attach one supported AirBridge adapter.' >&2; exit 2; }
usb_path=$selected_usb
usb=$(readlink -f "$usb_path")
for usbif in "$usb_path":*; do
    [[ -d "$usbif" ]] || continue
    if [[ ! -L "$usbif/driver" ]]; then
        printf '%s' "${usbif##*/}" > /sys/bus/usb/drivers/rtl8xxxu/bind
    fi
done
for attempt in {1..50}; do
    for net in /sys/class/net/*; do
        [[ $(readlink -f "$net/device/.." 2>/dev/null || true) == "$usb" ]] || continue
        radio=${net##*/}
    done
    [[ -n "$radio" ]] && break
    sleep 0.2
done
[[ -n "$radio" ]] || { echo "The adapter did not create a wireless interface."; dmesg | tail -20; exit 3; }
iw dev "$radio" info
iw phy
ip link set "$radio" down
iw dev "$radio" set type monitor
if iw dev "$radio" set monitor active; then
    echo "ACTIVE_MONITOR_REQUEST_ACCEPTED"
else
    echo "ACTIVE_MONITOR_REQUEST_REJECTED"
    iw dev "$radio" set monitor none
fi
ip link set "$radio" up
iw dev "$radio" set channel 6 HT20
echo "MONITOR_RADIO_STARTED"
iw dev "$radio" info
timeout --signal=INT "$duration" "$runtime/owl/build/daemon/owl" -i "$radio" -c 6 -N -f -v || test $? == 124

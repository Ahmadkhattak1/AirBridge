#!/usr/bin/env bash
# AirBridge native AirDrop radio runtime for WSL2.
set -euo pipefail

root=/opt/airbridge
mode=${1:-receive}
destination=${2:-/mnt/c/Users/Public/Downloads/AirBridge}
name=${3:-AirBridge}
send_files=()
if [[ "$mode" == send ]]; then
    name=${2:-Windows PC}
    shift 2
    send_files=("$@")
    [[ ${#send_files[@]} -gt 0 ]] || { echo 'No files selected' >&2; exit 2; }
fi
source "$root/bin/adapter-profile.sh"
run=/run/airbridge
logs=/var/log/airbridge
if [[ "$mode" == stop ]]; then
    [[ -f "$run/session.pid" ]] || exit 0
    read -r pid started < "$run/session.pid"
    [[ "$pid" =~ ^[0-9]+$ && -r "/proc/$pid/stat" ]] || exit 0
    [[ $(awk '{print $22}' "/proc/$pid/stat") == "$started" ]] || exit 0
    [[ $(tr '\0' ' ' < "/proc/$pid/cmdline") == *'/opt/airbridge/bin/airbridge-radio '* ]] || exit 0
    kill -TERM "$pid"
    # Restoring the USB driver can take several seconds after the helpers exit.
    for _ in {1..200}; do
        kill -0 "$pid" 2>/dev/null || exit 0
        sleep 0.1
    done
    echo 'AirBridge cleanup is still running' >&2
    exit 1
fi
[[ "$mode" == receive || "$mode" == probe || "$mode" == prepare || "$mode" == send ]] || exit 2
mkdir -p "$run" "$logs" /mnt/wsl/airbridge-firmware
[[ "$mode" == receive || "$mode" == probe ]] && mkdir -p "$destination"

exec 9>"$run/session.lock"
flock -n 9 || { echo '{"event":"error","message":"AirBridge is already running"}'; exit 75; }
printf '%s %s\n' "$$" "$(awk '{print $22}' /proc/$$/stat)" > "$run/session.pid"

radio=
owl_pid=
receiver_pid=
publisher_pid=
avahi_pid=
hostapd_pid=
tail_pid=
monitor_created=
ap_created=
cck_created=
experiment_loaded=
firmware_parameter=/sys/module/firmware_class/parameters/path
firmware_original=$(cat "$firmware_parameter")

cleanup() {
    trap - EXIT
    # Multiple Windows shutdown helpers can overlap. A second TERM must not
    # kill this shell halfway through restoring the radio and removing state.
    trap '' INT TERM
    local p
    for p in "$tail_pid" "$publisher_pid" "$receiver_pid" "$owl_pid" "$avahi_pid" "$hostapd_pid"; do
        [[ -z "$p" ]] || kill "$p" 2>/dev/null || true
    done
    for _ in {1..30}; do
        local alive=
        for p in "$tail_pid" "$publisher_pid" "$receiver_pid" "$owl_pid" "$avahi_pid" "$hostapd_pid"; do
            [[ -z "$p" ]] || ! kill -0 "$p" 2>/dev/null || alive=1
        done
        [[ -z "$alive" ]] && break
        sleep 0.1
    done
    for p in "$tail_pid" "$publisher_pid" "$receiver_pid" "$owl_pid" "$avahi_pid" "$hostapd_pid"; do
        [[ -z "$p" ]] || kill -KILL "$p" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    [[ -z "$monitor_created" ]] || iw dev abmon0 del 2>/dev/null || true
    [[ -z "$ap_created" ]] || iw dev abap0 del 2>/dev/null || true
    [[ -z "$cck_created" ]] || iw dev abcck0 del 2>/dev/null || true
    if [[ -n "$radio" && -e "/sys/class/net/$radio" ]]; then
        ip link set "$radio" down 2>/dev/null || true
        iw dev "$radio" set type managed 2>/dev/null || true
    fi
    if [[ -n "$experiment_loaded" ]]; then
        [[ ! -e /sys/module/rtl8xxxu/parameters/airbridge_cck ]] || printf N > /sys/module/rtl8xxxu/parameters/airbridge_cck
        if [[ ! -f /sys/module/rtl8xxxu/parameters/airbridge_inject_rate ]]; then
            if ! grep -q '^rtl8xxxu ' /proc/modules || rmmod rtl8xxxu; then
                insmod "$root/modules/rtl8xxxu.ko" || true
            fi
        fi
    fi
    printf '%s\n' "$firmware_original" > "$firmware_parameter"
    rm -f "$run/session.pid" "$run/radio.ready" "$run/approval" "$run/send-target"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# usbipd can report success before the kernel has enumerated the device.
# Keep this WSL session alive through that delay instead of failing immediately.
for _ in {1..50}; do
    if select_adapter "$root/adapters.tsv" /sys/bus/usb/devices; then break; else status=$?; fi
    [[ "$status" == 1 ]] || exit "$status"
    sleep 0.1
done
[[ -n ${usb_id:-} ]] || {
    echo '{"event":"error","message":"A supported AirBridge adapter is not attached"}'
    exit 2
}
[[ -f "$root/firmware/$firmware" ]] || { echo "The $chipset firmware is missing. Repair AirBridge setup." >&2; exit 3; }
echo "{\"event\":\"adapter-profile\",\"usb_id\":\"$usb_id\",\"chipset\":\"$chipset\",\"driver\":\"$driver\"}"

cp -au "$root/firmware/." /mnt/wsl/airbridge-firmware/
cp -u /lib/firmware/regulatory.db /mnt/wsl/airbridge-firmware/
[[ ! -f /lib/firmware/regulatory.db.p7s ]] || cp -u /lib/firmware/regulatory.db.p7s /mnt/wsl/airbridge-firmware/
printf '%s' /mnt/wsl/airbridge-firmware > "$firmware_parameter"

modprobe cfg80211
modprobe mac80211
modprobe led-class
if [[ "$mode" == send || ${AIRBRIDGE_INJECT_RATE:-1} == 1 ]] && [[ -f "$root/modules/rtl8xxxu-awdl.ko" ]]; then
    if [[ ! -f /sys/module/rtl8xxxu/parameters/airbridge_cck || ! -f /sys/module/rtl8xxxu/parameters/airbridge_inject_rate ]]; then
        if grep -q '^rtl8xxxu ' /proc/modules; then rmmod rtl8xxxu; fi
        insmod "$root/modules/rtl8xxxu-awdl.ko" airbridge_inject_rate="${AIRBRIDGE_INJECT_RATE:-1}"
    fi
    printf '%s' "${AIRBRIDGE_INJECT_RATE:-1}" > /sys/module/rtl8xxxu/parameters/airbridge_inject_rate
    echo "{\"event\":\"radio-driver\",\"injection_rate\":${AIRBRIDGE_INJECT_RATE:-1}}"
elif [[ "$mode" == send ]]; then
    echo 'The sender wake driver is missing. Repair AirBridge setup.' >&2
    exit 3
else
    if [[ -f /sys/module/rtl8xxxu/parameters/airbridge_inject_rate ]]; then rmmod rtl8xxxu; fi
    if ! grep -q '^rtl8xxxu ' /proc/modules; then insmod "$root/modules/rtl8xxxu.ko"; fi
fi

usb=$selected_usb
for usbif in "$usb":*; do
    [[ -d "$usbif" ]] || continue
    if [[ ! -L "$usbif/driver" ]]; then
        printf '%s' "${usbif##*/}" > /sys/bus/usb/drivers/rtl8xxxu/bind
    fi
done

for _ in {1..50}; do
    for net in /sys/class/net/*; do
        [[ $(readlink -f "$net/device/.." 2>/dev/null || true) == $(readlink -f "$usb") ]] || continue
        radio=${net##*/}
    done
    [[ -n "$radio" ]] && break
    sleep 0.2
done
[[ -n "$radio" ]] || {
    echo 'The Wi-Fi adapter did not power on. Unplug it, then reconnect it.' >&2
    dmesg | tail -n 20 >&2
    exit 3
}

if [[ "$mode" == prepare ]]; then
    echo '{"event":"adapter-ready"}'
    exit 0
fi

ip link set "$radio" down
iw dev "$radio" set type managed
phy=$(basename "$(readlink -f "/sys/class/net/$radio/phy80211")")
mac=$(cat "/sys/class/net/$radio/address")
rx_streams=1
phy_info=$(iw phy "$phy" info)
channel=$(awdl_channel <<< "$phy_info") || {
    echo 'No permitted AirBridge transfer channel is available on this adapter.' >&2
    exit 3
}
hw_mode=g
[[ "$channel" == 6 ]] || hw_mode=a
if grep -E 'HT RX MCS rate indexes supported:.*0-15' <<< "$phy_info" >/dev/null; then
    rx_streams=2
fi
if [[ "$mode" == send ]]; then
    [[ -f /sys/module/rtl8xxxu/parameters/airbridge_cck ]] || { echo 'The sender wake driver is missing. Repair AirBridge setup.' >&2; exit 3; }
    printf Y > /sys/module/rtl8xxxu/parameters/airbridge_cck
    # Always restore normal transmission on cancellation during the wake phase.
    experiment_loaded=1
    iw phy "$phy" interface add abcck0 type monitor
    cck_created=1
    ip link set abcck0 address "$mac"
    ip link set abcck0 up
    iw dev abcck0 set channel 4 HT20
    "$root/bin/airbridge-radio-node" --monitor abcck0 --wake &
    publisher_pid=$!
    wait "$publisher_pid"
    publisher_pid=
    iw dev abcck0 del
    cck_created=
    printf N > /sys/module/rtl8xxxu/parameters/airbridge_cck
    experiment_loaded=
fi
# rtl8xxxu has no active monitor or IBSS. A private hidden AP supplies
# hardware ACKs while a separate monitor carries AWDL. No IP network is hosted.
iw phy "$phy" interface add abap0 type __ap
ap_created=1
ip link set abap0 address "$mac"
printf 1 > /proc/sys/net/ipv6/conf/abap0/disable_ipv6
secret=$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')
umask 077
cat > "$run/hostapd.conf" <<EOF
interface=abap0
driver=nl80211
ssid=AirBridge-radio
ignore_broadcast_ssid=1
hw_mode=$hw_mode
channel=$channel
ieee80211n=1
beacon_int=1000
wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=$secret
EOF
hostapd "$run/hostapd.conf" >"$logs/hostapd.log" 2>&1 &
hostapd_pid=$!
for _ in {1..50}; do
    kill -0 "$hostapd_pid" 2>/dev/null || { cat "$logs/hostapd.log" >&2; exit 4; }
    grep -q 'AP-ENABLED' "$logs/hostapd.log" && break
    sleep 0.1
done
grep -q 'AP-ENABLED' "$logs/hostapd.log" || { cat "$logs/hostapd.log" >&2; exit 4; }
iw phy "$phy" interface add abmon0 type monitor
monitor_created=1
ip link set abmon0 address "$mac"
ip link set abmon0 up

rm -f "$run/radio.ready" "$run/approval"
port=8770
[[ "$mode" != send ]] || port=0
node_args=()
node_args+=(--tx-rate "${AIRBRIDGE_TX_RATE:-$tx_rate}")
node_args+=(--channel "$channel" --rx-streams "$rx_streams")
if [[ "$mode" == receive && ${AIRBRIDGE_RX_REPLY:-1} == 1 ]]; then
    node_args+=(--rx-reply)
fi
if [[ "$mode" == receive && ${AIRBRIDGE_RX_AMPDU:-1} == 1 ]]; then
    node_args+=(--rx-ampdu)
fi
[[ "$mode" == receive ]] || node_args+=(--browse)
RUST_LOG=info "$root/bin/airbridge-radio-node" --monitor abmon0 --name "$name" --port "$port" --ready-file "$run/radio.ready" "${node_args[@]}" >"$logs/radio.log" 2>&1 &
owl_pid=$!
for _ in {1..50}; do
    kill -0 "$owl_pid" 2>/dev/null || { tail -n 40 "$logs/radio.log" >&2; exit 4; }
    [[ -f "$run/radio.ready" ]] && break
    sleep 0.2
done
[[ -f "$run/radio.ready" ]] || { echo 'The AirDrop radio did not start' >&2; exit 4; }
echo "{\"event\":\"radio-ready\",\"interface\":\"$radio\"}"

if [[ "$mode" == probe ]]; then
    seconds=${AIRBRIDGE_PROBE_SECONDS:-30}
    [[ "$seconds" =~ ^[0-9]+$ && "$seconds" -ge 1 && "$seconds" -le 120 ]] || exit 2
    timeout --signal=INT "$seconds" tail -f "$logs/radio.log" &
    tail_pid=$!
    wait "$tail_pid" || true
    exit 0
fi

wait_for_transfer() {
    while kill -0 "$receiver_pid" 2>/dev/null; do
        if [[ ! -d /sys/class/net/abmon0 ]] ||
            ! kill -0 "$owl_pid" 2>/dev/null || ! kill -0 "$hostapd_pid" 2>/dev/null; then
            echo '{"event":"error","message":"The Wi-Fi adapter disconnected. Reconnect it to try again."}'
            return 6
        fi
        sleep 0.5
    done
    wait "$receiver_pid"
}

if [[ "$mode" == send ]]; then
    rm -f "$run/send-target"
    echo '{"event":"searching","message":"Looking for nearby AirDrop receivers"}'
    "$root/bin/airbridge" choose-send --radio-log "$logs/radio.log" --selection-file "$run/send-target" --name "$name" "${send_files[@]}" &
    receiver_pid=$!
    if wait_for_transfer; then exit 0; else exit $?; fi
fi

address=$(ip -6 -o addr show dev awdl0 scope link | awk '{print $4}' | cut -d/ -f1)
ifindex=$(cat /sys/class/net/awdl0/ifindex)
"$root/bin/airbridge" receive --listen "[$address%$ifindex]:8770" --dir "$destination" --name "$name" --approval-file "$run/approval" >"$logs/receiver.log" 2>&1 &
receiver_pid=$!
for _ in {1..50}; do
    kill -0 "$receiver_pid" 2>/dev/null || { cat "$logs/receiver.log" >&2; exit 5; }
    grep -q '"event":"ready"' "$logs/receiver.log" && break
    sleep 0.1
done
grep -q '"event":"ready"' "$logs/receiver.log" || exit 5

# Forward machine-readable receiver events to the Windows controller.
tail -q -n +1 -f "$logs/receiver.log" "$logs/radio.log" &
tail_pid=$!
wait_for_transfer
echo '{"event":"error","message":"An AirBridge radio or receiver process stopped"}'
exit 1

#!/usr/bin/env bash
# Shared catalog selection. Keep this independent of radio/service mutations.
select_adapter() {
    local catalog=$1 devices=$2 usb id profile
    local -a matches=() profiles=()
    unset selected_usb usb_id chipset driver firmware tx_rate
    [[ -r "$catalog" ]] || { echo 'The adapter catalog is missing. Repair AirBridge setup.' >&2; return 2; }
    for usb in "$devices"/*; do
        [[ -f "$usb/idVendor" && -f "$usb/idProduct" ]] || continue
        id="$(cat "$usb/idVendor"):$(cat "$usb/idProduct")"
        profile=$(awk -F '\t' -v id="$id" '$1 == id { sub(/\r$/, ""); print; exit }' "$catalog")
        [[ -n "$profile" ]] || continue
        matches+=("$usb")
        profiles+=("$profile")
    done
    if [[ ${#matches[@]} -gt 1 ]]; then
        echo 'More than one supported adapter is attached. Unplug the extra adapter.' >&2
        return 2
    fi
    [[ ${#matches[@]} -eq 1 ]] || return 1
    selected_usb=${matches[0]}
    IFS=$'\t' read -r usb_id chipset driver firmware tx_rate <<< "${profiles[0]}"
    [[ "$usb_id" =~ ^[0-9a-f]{4}:[0-9a-f]{4}$ && "$driver" == rtl8xxxu &&
        "$firmware" =~ ^rtlwifi/[a-zA-Z0-9_]+\.bin$ && "$tx_rate" =~ ^mcs[0-9]+$ ]] || {
        echo 'Invalid adapter profile. Repair AirBridge setup.' >&2
        return 2
    }
}

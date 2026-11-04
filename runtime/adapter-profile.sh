#!/usr/bin/env bash
# Shared catalog selection. Keep this independent of radio/service mutations.
# Only use frequencies iw explicitly lists for initiating an HT20 AP. Its
# unrestricted frequency lines end after the transmit power in dBm.
awdl_channel() {
    awk '
        NF == 6 && $1 == "*" && $3 == "MHz" && $6 == "dBm)" {
            if ($2 == 5745 && $4 == "[149]") high = 1
            if ($2 == 2437 && $4 == "[6]") low = 1
        }
        END {
            if (high) print 149
            else if (low) print 6
            else exit 1
        }'
}

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

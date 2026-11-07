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

# Read HT receive capabilities from the band containing our chosen channel.
# Do not infer them from a chipset name or from the requested injection rate.
# Output: capability word, A-MPDU parameters, MCS 0-31 receive mask (hex).
awdl_ht_profile() {
    awk -v channel="$1" '
        function hex(s,    i,n,d) {
            sub(/^0x/, "", s); sub(/[^0-9a-fA-F].*$/, "", s)
            n = 0
            for (i = 1; i <= length(s); i++) {
                d = index("0123456789abcdef", tolower(substr(s, i, 1))) - 1
                if (d < 0) return -1
                n = n * 16 + d
            }
            return length(s) ? n : -1
        }
        function emit() {
            if (selected && cap >= 0 && exponent >= 0 && spacing >= 0 && mcs > 0) {
                printf "%04x %02x %08x\n", cap, exponent + 4 * spacing, mcs
                found = 1
            }
        }
        /^[ \t]*Band [0-9]+:/ {
            emit(); cap = exponent = spacing = -1; mcs = selected = 0
        }
        /^[ \t]*Capabilities: 0x/ { cap = hex($2) }
        /Maximum RX AMPDU length.*exponent:/ {
            value = $NF; sub(/\).*/, "", value); exponent = hex(value)
            if (exponent > 3) exponent = -1
        }
        /Minimum RX AMPDU time spacing:/ {
            value = $NF; sub(/^\(/, "", value); sub(/\).*/, "", value); spacing = hex(value)
            if (spacing > 7) spacing = -1
        }
        /HT (TX\/RX|RX) MCS rate indexes supported:/ {
            line = $0; sub(/^.*supported:[ \t]*/, "", line)
            count = split(line, ranges, /,[ \t]*/)
            for (i = 1; i <= count; i++) {
                split(ranges[i], ends, "-")
                lo = ends[1] + 0; hi = ends[2] == "" ? lo : ends[2] + 0
                if (ranges[i] !~ /^[0-9]+(-[0-9]+)?$/ || lo > hi) continue
                for (bit = lo; bit <= hi && bit < 32; bit++) {
                    if (!seen[bit]) { mcs += 2 ^ bit; seen[bit] = 1 }
                }
            }
        }
        $1 == "*" && $3 == "MHz" && $4 == "[" channel "]" { selected = 1 }
        /^[ \t]*Band [0-9]+:/ { delete seen }
        END { emit(); if (!found) exit 1 }'
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

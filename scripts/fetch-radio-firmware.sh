#!/usr/bin/env bash
# Fetch exactly the redistributable firmware used by the adapter catalog.
set -euo pipefail
destination=${1:-/opt/airbridge/firmware/rtlwifi}
revision=0cb5a7fb037ef9489deaa4ebecd78b40fe7c07c6
base=https://kernel.googlesource.com/pub/scm/linux/kernel/git/firmware/linux-firmware
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
while read -r checksum file; do
    curl --fail --silent --show-error --location --retry 3 \
        "$base/+/$revision/rtlwifi/$file?format=TEXT" | base64 -d > "$stage/$file"
    printf '%s  %s\n' "$checksum" "$stage/$file" | sha256sum --check --status
done <<'FIRMWARE'
2ff74315287529dec2e50eb57d6e0c97d2116f28ae166773ccdf93b6360000c4 rtl8188eufw.bin
e6189d6d7032b83f0ed4284629c43db66262449a866f33fd329e611239f62e1f rtl8188fufw.bin
b15bc955fba2fc3abc093affe62c9f7284a0b84d4f13f8ce55366488dc9aad8b rtl8192eu_nic.bin
3001f8b50176ede76d0cde20c7c7ff84ebe12331049e65e1a1a768b1d45b74de rtl8192fufw.bin
FIRMWARE
# Validate the complete set before replacing any installed firmware.
mkdir -p "$destination"
for file in "$stage"/*.bin; do
    install -m644 "$file" "$destination/$(basename "$file")"
done
echo AIRBRIDGE_RADIO_FIRMWARE_INSTALLED

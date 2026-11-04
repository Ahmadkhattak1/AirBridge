#!/usr/bin/env bash
# Run descriptor regression checks against the exact supported kernel source.
set -euo pipefail
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
revision=c21a03b2943d147c280bdf32530d4fe6badfd6bd
relative=drivers/net/wireless/realtek/rtl8xxxu
base=https://raw.githubusercontent.com/microsoft/WSL2-Linux-Kernel
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
mkdir -p "$stage/$relative"
while read -r checksum file; do
    curl --fail --silent --show-error --location --retry 3 \
        "$base/$revision/$relative/$file" -o "$stage/$relative/$file"
    printf '%s  %s\n' "$checksum" "$stage/$relative/$file" | sha256sum --check --status
done <<'SOURCES'
295703a4cc0268b038eecb2b1d3dc94f99636aef58e0def81208d1f379d2c446 core.c
8e2ccaeaacde1be5b3c5fa359107ce6c5cbde5f967acfbfa463e5ff62a4646b1 rtl8xxxu.h
SOURCES
patch --batch --fuzz=0 -d "$stage" -p1 < "$source_root/runtime/rtl8188f-awdl.patch"
python3 "$source_root/scripts/test-driver-descriptors.py" "$stage/$relative"

#!/usr/bin/env bash
# Package only runtime files. Never export the developer distribution.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:?Usage: package-runtime.sh OUTPUT_DIRECTORY}
mkdir -p "$output"
stage=$(mktemp -d /var/cache/airbridge-release-overlay.XXXXXX)
trap '[[ "$stage" == /var/cache/airbridge-release-overlay.* ]] && rm -rf -- "$stage"' EXIT
kernel=$(uname -r)
[[ "$kernel" == 6.18.33.2-microsoft-standard-WSL2 ]] || { echo 'Unsupported build kernel' >&2; exit 1; }
mkdir -p "$stage/opt/airbridge/bin" "$stage/opt/airbridge/modules" "$stage/opt/airbridge/firmware" "$stage/usr/lib/modules/$kernel"
target=${CARGO_TARGET_DIR:-$root/target}
install -m755 "$target/release/airbridge" "$stage/opt/airbridge/bin/airbridge"
install -m755 "$target/release/airbridge-radio" "$stage/opt/airbridge/bin/airbridge-radio-node"
install -m755 "$root/runtime/airbridge-radio.sh" "$stage/opt/airbridge/bin/airbridge-radio"
install -m644 "$root/runtime/adapter-profile.sh" "$stage/opt/airbridge/bin/adapter-profile.sh"
install -m644 "$root/runtime/adapters.tsv" "$stage/opt/airbridge/adapters.tsv"
install -m755 "$root/runtime/bootstrap.sh" "$stage/opt/airbridge/bin/bootstrap"
modinfo -p /opt/airbridge/modules/rtl8xxxu-awdl.ko | grep '^airbridge_cck:' >/dev/null || {
    echo 'Build the shared driver before packaging.' >&2; exit 1;
}
for module in rtl8xxxu rtl8xxxu-awdl rtl8xxxu-cck-experiment; do
    install -m644 "/opt/airbridge/modules/$module.ko" "$stage/opt/airbridge/modules/"
    strip --strip-debug "$stage/opt/airbridge/modules/$module.ko"
done
for module in cfg80211 mac80211 libarc4 led-class tun; do
    file=$(modinfo -n "$module")
    destination="$stage/usr/lib/modules/$kernel/${file#*/modules/$kernel/}"
    install -Dm644 "$file" "$destination"
    strip --strip-debug "$destination"
done
cp -a /opt/airbridge/firmware/. "$stage/opt/airbridge/firmware/"
while IFS=$'\t' read -r usb_id chipset driver firmware rate; do
    [[ "$usb_id" != \#* && -n "$usb_id" ]] || continue
    test -f "$stage/opt/airbridge/firmware/$firmware" || { echo "Missing firmware for $chipset" >&2; exit 1; }
done < "$root/runtime/adapters.tsv"
cp -a "$root/runtime/licenses" "$stage/opt/airbridge/"
# The kernel requests firmware in the host namespace, hence the shared /mnt/wsl path.
install -Dm644 /lib/firmware/regulatory.db "$stage/usr/lib/firmware/regulatory.db"
if [[ -f /lib/firmware/regulatory.db.p7s ]]; then
    install -Dm644 /lib/firmware/regulatory.db.p7s "$stage/usr/lib/firmware/regulatory.db.p7s"
fi
install -Dm644 "$root/THIRD-PARTY-NOTICES.md" "$stage/opt/airbridge/THIRD-PARTY-NOTICES.md"
find "$stage" -type f -name '*.ko' -exec modinfo -F vermagic {} \; | grep -v "^$kernel " && exit 1
tar -C "$stage" --sort=name --owner=0 --group=0 -czf "$output/airbridge-runtime.tar.gz" .
echo "$output/airbridge-runtime.tar.gz"

#!/usr/bin/env bash
# Run inside the user's newly imported Ubuntu base, never the developer distro.
set -euo pipefail
[[ $(id -u) == 0 ]] || { echo 'Runtime setup requires root inside WSL.' >&2; exit 1; }
export DEBIAN_FRONTEND=noninteractive
printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d
chmod 755 /usr/sbin/policy-rc.d
apt-get update
apt-get install -y --no-install-recommends bash ca-certificates coreutils grep sed gawk iproute2 iw hostapd kmod util-linux procps
apt-get clean
depmod -a
install -d /var/log/airbridge
chmod 700 /var/log/airbridge
printf '[user]\ndefault=root\n[boot]\nsystemd=false\n' > /etc/wsl.conf
printf 'AirBridge runtime 0.3.0\n' > /opt/airbridge/runtime-version
echo AIRBRIDGE_SETUP_COMPLETE

#!/usr/bin/env python3
"""Bounded, opt-in channel-4 probe. No file contents are transmitted."""
import argparse
import json
from pathlib import Path
import socket
import struct
import subprocess
import time

import numpy as np
from waveform import bits, cck_bytes, codebook, decode_model, packet, airdrop_pdu, synthesize, unscramble, self_test


def frames(style, rotation, mac):
    values = packet(airdrop_pdu())
    params, wave, start = synthesize(values, style=style, start_phase=rotation*np.pi/8)
    recovered = decode_model(wave, len(values), start, cutoff_mhz=.6)
    assert min(np.count_nonzero(x[1] != values) for x in recovered) == 0
    desired = cck_bytes(params, first_symbol=32)
    # Radiotap RATE=11Mbps, TX_FLAGS=NOACK|NOSEQNO. Driver experiment marker
    # enforces that rate and disables sequence rewriting for only these frames.
    radiotap = struct.pack('<BBHI', 0, 0, 12, (1 << 2) | (1 << 15)) + bytes([22, 0]) + struct.pack('<H', 0x18)
    header = b'\xd0\x00\x00\x00' + b'\xff'*6 + mac + b'\xff'*6 + b'\x00\x00'
    marker = b'\x7f\x00\xe0\x25ABW1'
    assert len(header + marker) == 32
    return [radiotap + header + marker + unscramble(desired, seed) for seed in range(128)]


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--interface', required=True)
    p.add_argument('--seconds', type=int, default=20)
    p.add_argument('--style', choices=['smooth', 'step'], default='step')
    p.add_argument('--rotation', type=int, choices=range(4), default=0)
    args = p.parse_args()
    if not 1 <= args.seconds <= 30:
        p.error('each transmission experiment is limited to 1–30 seconds')
    enabled = Path('/sys/module/rtl8xxxu/parameters/airbridge_cck')
    if not enabled.exists() or enabled.read_text().strip() != 'Y':
        p.error('the opt-in AirBridge CCK test driver must be enabled')
    info = subprocess.check_output(['iw', 'dev', args.interface, 'info'], text=True)
    if 'type monitor' not in info or 'channel 4 (2427 MHz)' not in info:
        p.error('requires an isolated monitor interface on Wi-Fi channel 4')
    mac = bytes.fromhex(Path(f'/sys/class/net/{args.interface}/address').read_text().strip().replace(':', ''))
    self_test()
    batch = frames(args.style, args.rotation, mac)
    sock = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3))
    sock.bind((args.interface, 0))
    sock.settimeout(.2)
    start = time.monotonic()
    count = 0
    try:
        while time.monotonic() - start < args.seconds:
            sock.send(batch[count % len(batch)])
            count += 1
            # Approx. 200 short frames/sec; retain normal Wi-Fi carrier sensing.
            time.sleep(.005)
    finally:
        sock.close()
    print(json.dumps({'event': 'cck-probe-submitted', 'packets': count,
        'seconds': time.monotonic() - start, 'style': args.style, 'rotation': args.rotation,
        'wifi_channel': 4, 'intended_ble_channel': 38, 'iphone_reception_verified': False}), flush=True)


if __name__ == '__main__':
    main()

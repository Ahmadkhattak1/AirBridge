#!/usr/bin/env python3
"""Offline Wi-Fi/CCK to BLE announcement experiment; never transmits.

Independent implementation of the modulation approach described by Liu et al.,
WiBeacon (MobiCom 2021). Not a port of their unavailable generator source.
BLE framing is checked against Bluetooth Core 5.4 Vol 6 Part C section 4.2.1.
The model does not establish that the RTL8188F or an iPhone receives this signal.
"""
import argparse
import json
from pathlib import Path

import numpy as np
from scipy.signal import firwin


def bits(data):
    return np.unpackbits(np.frombuffer(bytes(data), dtype=np.uint8), bitorder="little")


def octets(values):
    return np.packbits(np.asarray(values, dtype=np.uint8), bitorder="little").tobytes()


def crc_bits(pdu):
    state = 0x555555
    for bit in bits(pdu):
        feedback = ((state >> 23) & 1) ^ int(bit)
        state = (state << 1) & 0xffffff
        if feedback:
            state ^= 0x65b
    return np.array([(state >> n) & 1 for n in range(23, -1, -1)], dtype=np.uint8)


def whiten(values, channel=38):
    state = channel | 0x40
    out = []
    for bit in values:
        low = state & 1
        out.append(int(bit) ^ low)
        state >>= 1
        if low:
            state ^= 0x44
    return np.array(out, dtype=np.uint8)


def packet(pdu, channel=38):
    return np.concatenate((bits(bytes.fromhex("aad6be898e")),
                           whiten(np.concatenate((bits(pdu), crc_bits(pdu))), channel)))


def airdrop_pdu(tag=b"\x41\x42\x01", pdu_type=0):
    # Static random address reserved for this local experiment, little-endian.
    address = bytes.fromhex("2fff3525e0c0")
    manufacturer = b"\x4c\x00\x05\x12\x40" + tag + bytes(4) + b"\x03" + bytes(9)
    ad = b"\x02\x01\x06" + bytes((len(manufacturer) + 1, 0xff)) + manufacturer
    assert len(ad) <= 31
    payload = address + ad
    return bytes((0x40 | pdu_type, len(payload))) + payload


def codebook():
    # Absolute quarter-turn phases. Differential phi1 is inverted separately.
    params = np.array([[a, b, c, d] for a in range(4) for b in range(4)
                       for c in range(4) for d in range(4)], dtype=np.int16)
    a, b, c, d = params.T
    phases = np.array([a+b+c+d, a+c+d, a+b+d, a+d+2,
                       a+b+c, a+c, a+b+2, a]).T % 4
    return params, np.exp(1j * phases * np.pi / 2)


def synthesize(values, delta_mhz=1, style="smooth", start_phase=0):
    """Select legal CCK symbols approximating a BLE waveform at 11 Mchip/s."""
    padded = np.concatenate((np.tile([0, 1], 8), values, np.tile([0, 1], 8)))
    signs = 2 * padded.astype(float) - 1
    if style == "smooth":
        phase = np.cumsum(np.repeat(signs, 11)) * np.pi / 22
    else:
        phase = np.repeat(np.cumsum(signs) * np.pi / 2, 11)
    phase += start_phase
    phase -= np.arange(len(phase)) * (2 * np.pi * delta_mhz / 11)
    phase = np.pad(phase, (0, (-len(phase)) % 8), mode="edge")
    params, codes = codebook()
    code_phases = np.angle(codes)
    choices = []
    for desired in phase.reshape(-1, 8):
        distance = np.abs(np.angle(np.exp(1j * (code_phases - desired))))
        costs = distance.sum(axis=1)
        candidates = np.flatnonzero(costs < costs.min() + 1e-9)
        # Prefer separated large errors when the closest-phase choice ties.
        def score(index):
            bad = np.flatnonzero(distance[index] > np.pi / 2)
            return int(np.diff(bad).min()) if len(bad) > 1 else 8
        choice = max(candidates, key=score)
        choices.append(choice)
    return params[choices], codes[choices].reshape(-1), 16 * 11


def decode_model(wave, bit_count, start, delta_mhz=1, cutoff_mhz=.5):
    # Independent receiver: frequency translation, FIR, quadrature discriminator,
    # and integrate each BLE bit. The real phone's filter/AGC/timing are unknown.
    n = np.arange(len(wave))
    shifted = wave * np.exp(2j * np.pi * delta_mhz * n / 11)
    taps = firwin(65, cutoff_mhz, fs=11)
    filtered = np.convolve(shifted, taps, mode="same")
    discriminator = np.angle(filtered[1:] * np.conj(filtered[:-1]))
    offsets = range(-5, 6)
    outputs = []
    for offset in offsets:
        base = start + offset
        metric = np.array([discriminator[base+i*11:base+(i+1)*11].sum()
                           for i in range(bit_count)])
        outputs.append((offset, (metric > 0).astype(np.uint8), metric))
    return outputs


def cck_bytes(params, first_symbol=0, previous_phase=0, mapping="binary"):
    # d0,d1 use DQPSK Gray mapping; phi2..4 use binary mapping in 802.11b.
    # d0/d2/d4/d6 are transmitted first in each dibit.
    dqpsk_inverse = {0: 0, 1: 2, 2: 3, 3: 1}
    binary_inverse = {0: 0, 1: 2, 2: 1, 3: 3}
    phase_inverse = binary_inverse if mapping == "binary" else dqpsk_inverse
    out = []
    for index, (a, b, c, d) in enumerate(params):
        delta = (int(a) - previous_phase - 2 * ((first_symbol + index) % 2)) % 4
        out.append(dqpsk_inverse[delta] | (phase_inverse[int(b)] << 2)
                   | (phase_inverse[int(c)] << 4) | (phase_inverse[int(d)] << 6))
        previous_phase = int(a)
    return bytes(out)


def unscramble(desired, state):
    """Invert the DSSS feedback scrambler for one hypothesized initial state."""
    out = []
    for bit in bits(desired):
        out.append(int(bit) ^ ((state >> 3) & 1) ^ ((state >> 6) & 1))
        state = ((state << 1) | int(bit)) & 127
    return octets(out)


def self_test():
    pdu = bytes.fromhex("4209a6a5a4a3a2c1010203")
    expected = "01010101 01101011 01111101 10010001 01110001 00101001 00110011 01000111 10100001 10111111 10111110 11000010 01110010 01011000 11100101 00110101 11110111 11110011 10100101"
    assert "".join(map(str, packet(pdu))) == expected.replace(" ", ""), "Bluetooth SIG test vector"
    assert np.array_equal(whiten(whiten(bits(pdu))), bits(pdu))
    for state in range(128):
        source = bytes(range(256))
        plain = unscramble(source, state)
        actual = []
        for bit in bits(plain):
            b = int(bit) ^ ((state >> 3) & 1) ^ ((state >> 6) & 1)
            actual.append(b)
            state = ((state << 1) | b) & 127
        assert octets(actual) == source
    print("BLE framing/CRC/whitening matches Bluetooth SIG sample; scrambler inverse passes all 128 states.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    self_test()
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"experimental": True, "over_air_verified": False, "candidates": []}
    for style in ("smooth", "step"):
        for rotation in range(4):
            pdu = airdrop_pdu()
            values = packet(pdu)
            params, wave, start = synthesize(values, style=style, start_phase=rotation*np.pi/8)
            entry = {"style": style, "rotation": rotation, "ble_bits": len(values),
                     "cck_symbols": len(params), "filters": []}
            for cutoff in (.4, .5, .6, .75):
                candidates = decode_model(wave, len(values), start, cutoff_mhz=cutoff)
                best = min(candidates, key=lambda x: np.count_nonzero(x[1] != values))
                entry["filters"].append({"cutoff_mhz": cutoff, "offset": best[0],
                    "bit_errors": int(np.count_nonzero(best[1] != values)),
                    "minimum_decision_margin": float(np.min(np.abs(best[2])))})
            np.save(args.output / f"{style}-{rotation}-params.npy", params)
            entry["pdu_hex"] = pdu.hex()
            report["candidates"].append(entry)
            print(json.dumps(entry), flush=True)
    (args.output / "model.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()

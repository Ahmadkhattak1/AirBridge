# Throughput investigation, 2026-11-07

The original-video checkpoint is `6b4520b`, tagged
`checkpoint/2026-11-07-original-hdr`. This investigation does **not** establish
a substantial increase over its measured iPhone receive rate of 1.458 MB/s.

## Method and results

MacBookPro18,3 sent synthetic 8,388,608-byte files over its `awdl0` interface,
TLS/TCP, and the actual AirBridge Ask/Upload/archive path. Windows used the
RTL8188FTV (0bda:f179), channel 6, HT20. MB/s means decimal bytes/second.
The sender timer starts before the archive body and stops after the successful
HTTP response. It excludes discovery, approval and connection establishment.
The receiver's elapsed timer has a different start point; do not mix them.

| Comparison | Three sender rates, MB/s | Median |
| --- | --- | --- |
| Control, MCS7 acknowledgments | 0.573, 1.101, 1.022 | 1.022 |
| MCS3 acknowledgments | 0.968, 0.961, 0.978 | 0.968 |
| Linux timerfd instead of Tokio timer | 1.023, 0.794, 0.974 | 0.974 |
| Observed-presence ACK window, 8 TU instead of 1 | 1.028, 0.981, 0.965 | 0.981 |
| USB batches, 4 KiB threshold / 256 us flush | 1.015, 0.989, 0.863 | 0.989 |
| OFDM basic hardware response rates | 0.889, 1.139, 1.106 | 1.106 |

These short, sequential trials are subject to RF variation and are not a
statistical demonstration of improvement. None of those experimental changes
is retained. A stale-ACK replacement experiment eliminated only one ACK in an
8 MiB run, so that implementation was removed too. A metric-700 synchronization
master still got five shared slots; its file took 14.058 seconds at the receiver
and the sender did not receive completion before the harness timeout. The
normal metric 60 is restored. Runs labeled `ofdm-acks` used an older installed
runtime and are excluded; `ofdm-verified` checked the active hostapd settings.

All 27 saved synthetic files checked at the end of the trials had exactly
8,388,608 bytes and SHA-256
`7d212b9c884f5c77896de960ae17cc341cda43b14d6a971f34ca29ebd4badf7f`,
matching the sender. This integrity check includes the master experiment's
saved file; it does not turn its missing sender completion into a success.
These are Mac test results, not a new iPhone video speed measurement.

The final installed configuration completed one additional 8 MiB upload in
8.384 seconds (1.0005 MB/s); its saved checksum also matched. The final suites
passed 107 Linux and 105 Windows Rust tests, 27 required regression inventory
checks, the pinned-kernel driver harness, and both platforms' packaging tests.
Radio startup/cleanup and the installed binary hash were checked. This final
smoke test confirms operation and integrity, not a throughput improvement.

## What the lower-level measurements established

- The active sender commonly advertises channel 6 in 5 of 16 AWDL slots,
  and primary channel 149. The dongle remains on channel 6. The announced
  overlap is 31.25%; it is not itself a measurement of occupied airtime.
- Raw RTL8188F USB RX descriptors showed `paggr=1` on large Mac data frames:
  Wi-Fi A-MPDU aggregation is already active. They also showed both values
  of `splcp`, including short-GI reception. Lack of A-MPDU/short-GI metadata
  in ordinary radiotap capture must not be treated as proof of absence.
- An A-MSDU-in-A-MPDU negotiation trial returned the requested bit correctly,
  completed at 1.096 MB/s, but did not make the Mac use nested A-MSDUs. This
  single run is not evidence of a throughput gain from that negotiation.
- A 64 MiB write including `fsync` measured 38.88 MB/s through WSL into the
  Windows project folder and 69.46 MB/s in Linux `/tmp`. The Windows path is
  slower, but this test does not support it being a 1–1.5 MB/s receive limit.
  The user's unspecified Linux I/O pop-up has not been identified directly.
- Enabling the driver's existing `ht40_2g` option and requesting HT40+ through
  normal hostapd setup triggered `20/40 MHz operation not permitted ... based
  on overlapping BSSes`. The operating interface remained **20 MHz**. No
  coexistence override was used; the normal module configuration is restored.

For perspective, even 72.2 Mbit/s HT20 PHY rate multiplied by a 5/16 announced
overlap is only 2.82 MB/s **before** Wi-Fi headers, contention, retries, TCP/TLS
and switching overhead. This is an illustrative upper bound, not a measured
ceiling or proof that the remaining gap cannot be improved. Native 5 GHz Mac
results cannot be extrapolated to this 2.4 GHz, one-stream link.

## Retained code changes

1. The dedicated driver allocates at least 8192 bytes for receive frames,
   enough for the advertised HT A-MSDU plus headers and descriptor metadata.
   The old 2352-byte frame allocation could not hold those advertised sizes.
   Larger USB batching buffers and ordinary driver behavior are preserved.
2. Block Ack replies allow nested A-MSDUs only when the peer requests them
   and the runtime verifies the larger-buffer driver. This removes an
   unnecessary restriction without claiming the sender will use it.
3. ECN ECE/CWR flags no longer exclude otherwise eligible TCP data/ACKs from
   the existing single-use, same-flow, same-slot 1-TU reply path. Congestion
   flags are preserved unchanged; SYN/FIN/RST/URG remain excluded.
4. Schedule logs identify the selected synchronization master and metric.

Regression coverage checks full grouped-packet payload preservation, bounds,
Block Ack negotiation gating, ECN/control-flag discrimination, and both driver
descriptor layouts. The original media-negotiation and transfer-consent tests
remain required in CI.

Local diagnostic evidence is under ignored `output/test-2026-11-07/`, including
sender results, receiver/radio logs, packet captures, saved-file hashes and
the rejected experiment patches. USB batching register units were checked
against the [RTL8188F vendor driver](https://github.com/kelebek333/rtl8188fu/blob/master/hal/rtl8188f/usb/usb_halinit.c).

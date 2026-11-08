# Why this AirBridge link is slower than native AirDrop

Evidence reviewed November 8, 2026. New tuning experiments are paused while
the causes are separated. No new iPhone test is required for these findings.

## Main finding: different radio capacity, plus long off-channel intervals

The current RTL8188FTV is operating on **2.4 GHz channel 6, HT20, one spatial
stream**. The native Mac/iPhone session announced **5 GHz channel 149 in
15 of 16 availability slots**. In AirBridge sessions the sender commonly
announces channel 6 in only **5 of 16 slots**, while retaining channel 149
as its primary channel. The dongle cannot follow it to 149.

This is supported by actual packet timing, not just the announcements:

- In `mac-baseline-radio.pcap`, all **3,197** sampled large inbound TCP frames
  mapped to the sender's announced channel-6 windows.
- Four gaps lasted **660,698; 660,407; 660,506; and 665,264 microseconds**.
  Reconstructing the schedule from action frames and the receiver's hardware
  timestamps mapped **655–656 milliseconds of each gap to channel 149**.
  The remaining 4–10 ms is near channel transitions. Schedule integration
  used 1 ms samples, so these are not exact airtime measurements.
- A separate recent capture contained three 660–662 ms gaps occupying
  **62.84% of its 3.156-second data span**. This short capture is not a
  universal duty-cycle estimate, but it independently shows the long pauses.
- The native iPhone-to-Mac capture's large inbound UDP datagrams spanned
  **3.326275 seconds**, with a maximum gap of **95.911 ms**, and no gap of
  100 ms or more. This is an IP-interface capture, whereas the AirBridge
  captures use radio timestamps; neither measures RF occupancy directly.

For the current HT20/MCS0–7 receiver, the best PHY rate is approximately
65 Mbit/s with long GI or 72.2 Mbit/s with short GI. Even with continuous
availability, that is at most **8.125–9.025 MB/s before overhead**. Under a
5/16 shared-slot schedule, the illustrative ceiling becomes:

```
65 to 72.2 Mbit/s / 8 × 5/16 = 2.54 to 2.82 MB/s before overhead
```

This is a conditional capacity calculation, not a promised speed or a hard
limit on every dynamic schedule. It excludes Wi-Fi headers, contention,
retries, switching, TCP, TLS and archive overhead. HT40 is not active: the
normal coexistence check rejected it because of overlapping networks.

The original iPhone video arrived at **1.458 MB/s**. Native macOS reported
**35.03–46.80 MB/s** in successful AirDrop transfer metrics. The denominator
of that private metric has not been reverse engineered, so it is not an
exactly equivalent application timer. Independently, the native capture
contained 169,578,456 large-datagram payload bytes in 3.326275 seconds
(50.98 MB/s, including transport overhead/retransmissions; not file goodput).
The large capacity difference is real even without equating these timers.

The Mac's diagnostic **130 Mbit/s / MCS15 / channel 11** entry describes its
infrastructure connection `en0`, not AWDL. It must not be used as the native
AirDrop PHY rate. Native AWDL's exact per-packet PHY rate and stream count
remain unmeasured. Channel 149 being blocked by iPhone has never been shown.

## Software losses: measured, but smaller than the main gap

The following comparisons used the Mac sending the same synthetic files to
the actual Ask/Upload receiver. Timers ran on the Mac, independently of WSL.
Each row contains two 33,554,432-byte uploads. Aggregate rate is total bytes
divided by total seconds, not the average of rates.

| Configuration | Sender durations (s) | Aggregate MB/s |
| --- | --- | --- |
| Original adjusted-clock scheduling | 35.698643, 32.552191 | 0.983268 |
| Raw clock plus bounded transmit-delay correction | 33.524216, 33.524516 | 1.000897 |
| Same timing plus negotiated Wi-Fi receive reordering | 29.695486, 30.501221 | 1.114826 |

These are sequential small-sample experiments, not a randomized performance
guarantee or an iPhone improvement claim. The initial short-run timing gain
did not hold up in these longer runs: it fell to **1.79%**.

- **Clock error exists.** WSL's adjusted monotonic clock ran approximately
  4–5% slower than its raw clock during these comparisons. Mean absolute
  synchronization residual fell from **11.765 ms to 0.506 ms** after the
  timing correction. This uses counter deltas after the first log sample
  to exclude initial synchronization. The large timing improvement produced
  only a small throughput change, so timing is not the dominant rate limit.
- **Receive reordering is missing from the existing monitor path.** In one
  capture, 2,433 incoming frames contained 131 late sequence-gap repairs;
  median repair time was **4.911 ms**, and 112 repairs arrived within 16 ms.
  All tracked gaps eventually filled. Passing those frames straight to TCP
  exposes out-of-order arrival and duplicates. The experimental bounded
  reorder queue improved the longer sample by **11.38% versus timing alone**.
  This supports addressing receive ordering, but does not prove how many
  TCP congestion-window reductions it caused; that measurement remains open.
- Wi-Fi retry flags were present on **584 of 2,429** sampled large frames
  (**24.04%**) in that Mac capture. Retry flags are not a packet-loss rate.
  The separately measured iPhone sample had **7.12%** retry flags; do not
  apply the Mac percentage to the phone.

All **33** successful synthetic files from this investigation, totaling
427,819,008 bytes, were independently hashed on Windows and matched their
sender hashes and lengths. Experiments with hardware retries, forced TCP
quick acknowledgments, disabling aggregation, and channel 11 did not provide
a useful improvement. Channel 11 gave one shared slot and failed to complete
the benchmark; it is not enabled.

## What the evidence rules out as the main explanation

- **Disk/WSL file writing at 1 MB/s:** a fresh write check completed two
  separate 64 MiB writes, both including fsync, cleanup and process startup,
  in **0.896102 seconds measured by Windows**. Even attributing that entire
  duration to the Windows-folder write gives a conservative **74.89 MB/s**
  lower bound for that write in this test. It is not a sustained disk rating.
  Earlier TLS/archive loopback also exceeded the radio transfer rate by a
  large margin. The unspecified Linux I/O pop-up is still not identified.
- **Aggregation never enabled:** raw USB descriptors show incoming A-MPDU
  aggregation, and disabling it made the measured transfers slower.
- **A demonstrated QUIC-only speed fix:** native uses QUIC and AirBridge uses
  TCP, but changing transport cannot overcome the above radio capacity.
  Their separate contribution has not been isolated on an equal radio path.

## Next causal question, before more tuning

**Why does the Apple sender retain the 5 GHz schedule while communicating
with a 2.4 GHz-only AirBridge peer, and can a supported negotiation change
increase channel-6 availability?** That policy has not yet been established.
Simply changing an election metric or advertising channel 11 did not solve it.

To approach the measured native 35–47 MB/s, AirBridge also needs a radio and
driver supporting the native 5 GHz path with sufficient bandwidth and spatial
streams. A 5 GHz label alone is insufficient. The current shipped adapter
catalog covers 2.4 GHz rtl8xxxu families; native-speed 5 GHz interoperability
has not been validated. Receive reordering addresses a smaller, concrete
software loss and should be evaluated independently of that hardware work.

Evidence: ignored local `output/mac-measurements/findings.json`, native
`20261104T125039Z/awdl0.pcap`, `radio-watch.txt`, and
`output/test-2026-11-07/next/` (slot reconstruction, packet-gap and reorder
audits, host-timed storage check, and independently verified file manifest).
The paired source logs are `long-control`, `long-corrected`, and `long-reorder`.

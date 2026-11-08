# AirBridge 0.3.0

- Requests original HEVC HDR media from the iPhone. The tested 165 MB clip arrived with its original byte count and video properties.
- Adds a shared USB adapter catalog and reads the selected radio's actual capabilities. RTL8188FTV remains the physically verified adapter.
- Fixes receive buffer sizing for aggregated Wi-Fi frames and preserves TCP congestion feedback in the quick acknowledgment path.

The release uses the existing TCP transfer path by default. The [throughput diagnosis](docs/THROUGHPUT-DIAGNOSIS.md) records the measured radio limits and separate timing and reordering experiments.

## AirBridge 0.2.0

Windows setup installer for native iPhone AirDrop with the supported Realtek RTL8188FTV USB adapter (`0bda:f179`). See [installation instructions](docs/INSTALLATION.md).

- Installer supplies the radio runtime, verifies downloads, and sets up WSL and USB support when needed. A Windows restart may be required.
- Automatic adapter preparation, runtime keep-alive, and connection recovery between sessions.
- Compact connection menu at the top with copyable diagnostics and device details on failure.
- Streaming reception with an 8 GiB encoded/decoded bound, temporary staging, archive validation, and cleanup after incomplete uploads.
- Repeated offers for the same active transfer reuse approval. A different transfer cannot replace an upload in progress.
- Completion acknowledges a validated archive even when the iPhone is waiting to finish its HTTP body, fixing the end-of-video failure.
- Driver honors requested AWDL transmit rates and retry policy. The tested receive sample reached about 1.22 MB/s, with variation between runs.
- Windows and Linux CI and a required inventory of critical regression tests.

The receiver remains Everyone-style with explicit acceptance, without Apple Account/Contacts Only integration. Sending is limited to 512 MiB. Only the named adapter and the WSL kernel in the installer are supported. The installer is unsigned. Fresh Windows feature activation/reboot has not been exercised on a separate clean PC. The runtime is tested through an isolated fresh import on the development machine. Wider hardware/iOS compatibility and long-duration reliability remain areas for further testing.

The installer preserves existing WSL distributions and received files. It refuses to overwrite an unmanaged development runtime. See the installation guide for migration and removal details.

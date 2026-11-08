# AirBridge

Native Apple AirDrop for Windows. Download **AirBridge-0.3.0-Setup-x64.exe** from [Releases](https://github.com/Ahmadkhattak1/AirBridge/releases/latest). Setup installs the application and prepares its WSL radio runtime. A compatible USB Wi-Fi adapter is required. See [installation instructions](docs/INSTALLATION.md) and [release notes](RELEASE-NOTES.md).

Both directions have completed on the test machine with RTL8188FTV (`0bda:f179`). The runtime recognizes additional USB IDs listed in the [adapter catalog](runtime/adapters.tsv). Those adapters still need physical transfer testing.

## Current test machine

This checkout is configured for the connected Realtek RTL8188FTV USB Wi‑Fi adapter (`0bda:f179`). A dedicated WSL runtime supplies the AWDL radio interface that the normal Windows Realtek driver does not expose. The Windows application prepares the connection on launch, keeps the dedicated adapter attached while the app is open, and checks it automatically between sessions. Receive and Send start their radio sessions using that prepared connection.

Received files are saved to `Downloads\AirBridge`.

## Test the app

Install using Setup, then open AirBridge from the Start menu. The home screen has two actions: **Receive** and **Send files**. To receive, choose **Receive**, share from the iPhone using AirDrop, and accept in AirBridge. Developers can run `AirBridge.exe` in this checkout with the existing provisioned runtime.

To send, enable **Everyone for 10 Minutes** on the unlocked iPhone. Choose **Send files** and select files (or drop files onto the idle window). Discovery starts automatically. Allow about 20 seconds for the experimental wake step, then select your iPhone's device tile. Accept on the phone. **Cancel** or the back arrow stops the session. Sending and receiving use the dongle one session at a time.

The folder icon opens the save-location controls. You can open the folder, or choose a different local folder while stopped. Preferences are remembered in `%LOCALAPPDATA%\AirBridge\preferences.json`. Incoming byte counts come from the service; outgoing motion is an activity indication until the iPhone acknowledges the completed upload, not a made-up percentage.

For a visual-only demonstration, launch the application with `--preview`; its sample states never start the radio.

The connection menu at the top shows readiness and offers copyable diagnostics. Device details appear there when setup fails. After a disconnect, AirBridge checks and reconnects automatically. An adapter that fails to power on may still need to be unplugged and reconnected.

The sender temporarily uses a separately stored, opt-in CCK driver and restores the original driver during cleanup. Background preparation then selects the receive driver. This implements a Wi-Fi waveform experiment; there is no Bluetooth adapter in the tested setup. See `experiments/wifi_ble/README.md` for the method and evidence limits.

## Build

```powershell
.\Build.ps1
.\Build.ps1 -Release -SkipTests
```

The Windows UI binary is `target\release\airbridge-ui.exe`. The Rust protocol service is `target\release\airbridge.exe`.

## Components

- `airbridge-core`: binary-plist AirDrop messages, HTTP/1.1, gzip/DVZip and odc/newc transfer archives, validation, and receiver state.
- `airbridge-cli`: native TLS sender and receiver used inside the radio runtime.
- `airbridge-app`: the Windows UI and runtime controller.
- `airbridge-radio`: Rust AWDL driver integration and scoped IPv6 mDNS announcements, using pinned MIT protocol libraries.
- `runtime/airbridge-radio.sh`: adapter, hidden AP for hardware acknowledgments, monitor, and service orchestration. `stop` performs bounded cleanup of this session only.

## Device verification (27 October 2026)

- User confirmed AirBridge appeared in the iPhone's native AirDrop menu.
- iPhone sent a photo through Discover → Ask → UI acceptance → DVZip Upload. The first version caused HEIF-to-JPEG conversion on the phone. After updating the capabilities, another transfer saved `IMG_3150.HEIC` (1,376,048 bytes, `ftypheic` container), and the user confirmed the original HEIC/HEIF format arrived. Byte identity to the source has not been independently compared.
- The iPhone uploaded a video with a 37,805,732-byte DVZip body and the receiver saved its MOV file. Playback validation remains with the user.
- The radio receives iPhone AWDL data, exchanges IPv6 traffic, and carries real TLS sessions. This is over the air, distinct from earlier loopback-only tests.
- Runtime shutdown was checked to remove the AP/monitor/TAP interfaces and leave the physical dongle in managed mode.

## Current limits

- Incoming uploads stream through gzip/DVZip and CPIO validation into temporary files, with an 8 GiB bound on both the encoded body and decoded archive. Once the archive trailer and compression checks pass, the receiver saves the files and acknowledges the upload even if the sender is waiting to end the HTTP body. Incomplete archives are discarded. Sending is still limited to 512 MiB.
- Repeated offers from the same peer reuse approval only for the same active TransferID and file manifest. A second upload cannot replace a body in progress. Idle approvals expire after ten minutes; active uploads do not expire at a byte or time threshold.
- This RTL8188FTV only supports 2.4 GHz. AirDrop throughput on the current single-channel schedule is much lower than modern Apple-to-Apple transfers.
- The RTL8188FTV receiver accepts Block Ack requests so the iPhone can send packets in batches. Set `AIRBRIDGE_RX_AMPDU=0` to disable this for comparison. Outgoing unicast uses single-stream HT MCS7 when the peer advertises support, with a basic-rate fallback and bounded A-MSDU aggregation.
- The optional receiver driver honors the requested transmit rate and retry policy for AWDL frames, including TCP acknowledgments sent back to the iPhone. A 30-second sample of the same iPhone video reached 1.22 MB/s with the driver, compared with 0.35 MB/s in the preceding completed transfer. See [runtime prerequisites](docs/RUNTIME.md) for build instructions.
- Everyone-style discovery, with acceptance required on Windows; no Apple-ID Contacts Only integration.
- Sender behavior, repeated reconnects, interruption recovery, and long-duration reliability still require device tests.
- Reverse test: after a bounded Wi-Fi CCK wake experiment and updated transfer metadata, the iPhone accepted the offer, acknowledged the upload, and the user confirmed receiving the test file. Large reverse transfers and repeated user-driven UI transfers remain to be validated. Earlier TCP resets and incomplete uploads are historical failures, not the final result.

For the installed development runtime, build/update with `bash scripts/install-runtime.sh` from the WSL source directory after stopping AirBridge. The existing WSL kernel module and firmware must already be provisioned; this is not a general-purpose Windows installer yet.

Runtime diagnostics for the latest session are in `/var/log/airbridge` inside `AirBridgeRuntime` and survive a runtime reboot. Received files stay in the Windows Downloads folder.


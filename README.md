# AirBridge

Native Apple AirDrop for Windows. **Experimental developer preview: the Windows executable requires a separately provisioned WSL radio runtime; it is not a standalone installer.** Only USB `0bda:f179` is verified. Read [release notes](RELEASE-NOTES.md) and [runtime prerequisites](docs/RUNTIME.md) before downloading.

AirBridge is an experimental Windows application for native AirDrop. Both directions have completed on the test machine: iPhone-to-Windows photos/video and a Windows-to-iPhone text file. No iPhone app or web page is required. Broad compatibility and sustained reliability are not yet established.

## Current test machine

This checkout is configured for the connected Realtek RTL8188FTV USB Wi‑Fi adapter (`0bda:f179`). A dedicated WSL runtime supplies the AWDL radio interface that the normal Windows Realtek driver does not expose. The Windows application starts and stops that runtime automatically.

Received files are saved to `Downloads\AirBridge`.

## Test the app

Open `AirBridge.exe` in this folder. The home screen has two actions: **Receive** and **Send files**. To receive, choose **Receive**, share from the iPhone using AirDrop, and accept in AirBridge.

To send, enable **Everyone for 10 Minutes** on the unlocked iPhone. Choose **Send files** and select files (or drop files onto the idle window). Discovery starts automatically. Allow about 20 seconds for the experimental wake step, then select your iPhone's device tile. Accept on the phone. **Cancel** or the back arrow stops the session. Sending and receiving use the dongle one session at a time.

The folder icon opens the save-location controls. You can open the folder, or choose a different local folder while stopped. Preferences are remembered in `%LOCALAPPDATA%\AirBridge\preferences.json`. Incoming byte counts come from the service; outgoing motion is an activity indication until the iPhone acknowledges the completed upload, not a made-up percentage.

For a visual-only demonstration, launch `AirBridge.exe --preview`; its sample states never start the radio. The browser motion study is `output/ui-preview.html`. The previous executable is retained as `output/AirBridge-before-ui.exe`.

The sender temporarily uses a separately stored, opt-in CCK driver and restores the original receiver driver during cleanup. This implements a Wi-Fi waveform experiment; there is no Bluetooth adapter in the tested setup. See `experiments/wifi_ble/README.md` for the method and evidence limits.

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

- The receiver currently buffers an upload before saving it. The updated service caps the body and decoded archive at 512 MiB; streaming large videos remains required before a production release.
- This RTL8188FTV only supports 2.4 GHz. AirDrop throughput on the current single-channel schedule is much lower than modern Apple-to-Apple transfers.
- Everyone-style discovery, with acceptance required on Windows; no Apple-ID Contacts Only integration.
- Sender behavior, repeated reconnects, interruption recovery, and long-duration reliability still require device tests.
- Reverse test: after a bounded Wi-Fi CCK wake experiment and updated transfer metadata, the iPhone accepted the offer, acknowledged the upload, and the user confirmed receiving the test file. Large reverse transfers and repeated user-driven UI transfers remain to be validated. Earlier TCP resets and incomplete uploads are historical failures, not the final result.

For the installed development runtime, build/update with `bash scripts/install-runtime.sh` from the WSL source directory after stopping AirBridge. The existing WSL kernel module and firmware must already be provisioned; this is not a general-purpose Windows installer yet.

Runtime diagnostics are in `/run/airbridge/logs` inside `AirBridgeRuntime` and are cleared by a runtime reboot. Received files stay in the Windows Downloads folder.


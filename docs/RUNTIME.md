# Runtime prerequisites for the developer preview

This release does not yet provide a clean-machine runtime installer or distributable runtime image. The development machine has separately provisioned components; copying its EXE to another PC does not copy those components.

The Windows controller expects:

- Windows x64 with WSL2 and a distribution registered exactly as `AirBridgeRuntime`.
- usbipd-win available on PATH, with the supported adapter shared for USB passthrough. Attaching it gives the runtime exclusive use of that adapter during transfers.
- USB adapter `0bda:f179` (RTL8188FTV).

Inside that distribution, `runtime/airbridge-radio.sh` expects:

- Root access, Linux wireless support (`cfg80211`, `mac80211`, `led-class`), USB/IP and TUN/TAP.
- `iw`, `ip`, `hostapd`, `avahi-daemon`, `kmod`, `util-linux` and normal shell utilities.
- `/opt/airbridge/bin/airbridge`, `/opt/airbridge/bin/airbridge-radio-node` and the orchestration script at `/opt/airbridge/bin/airbridge-radio`.
- `/opt/airbridge/modules/rtl8xxxu.ko` (unmodified receiver driver), `/opt/airbridge/modules/rtl8xxxu-cck-experiment.ko` (opt-in sender experiment), and matching firmware under `/opt/airbridge/firmware`.
- Regulatory database files under `/lib/firmware`.

The tested driver build uses Microsoft WSL2 kernel tag `linux-msft-wsl-6.18.33.2`. Modules must match the running kernel and configuration. The sender change is in `experiments/wifi_ble/rtl8188f-cck-experiment.patch`; upstream source is https://github.com/microsoft/WSL2-Linux-Kernel/tree/linux-msft-wsl-6.18.33.2 . Build the original module separately before applying the patch. Do not replace the original receiver module with the experimental module.

On an already provisioned Linux runtime, `scripts/install-runtime.sh` builds and installs the Rust services and orchestration. It is an update script, not a prerequisite installer. It deliberately refuses to update during an active session.

For faster reception on the RTL8188FTV, stop AirBridge and run `bash scripts/build-receiver-driver.sh` as root inside the runtime. It builds the driver in a temporary directory using the matching, already built kernel tree in `/opt/airbridge/kernel` and installs `/opt/airbridge/modules/rtl8xxxu-awdl.ko`. Preparation selects that module when present and reuses it between receive sessions. `AIRBRIDGE_INJECT_RATE=0` selects the stock driver for comparison.

The app prepares the adapter on startup and keeps the runtime alive while open. Between sessions it checks the connection every five seconds, automatically attaches the adapter, and runs `airbridge-radio prepare` to verify the driver and interface. Preparation uses the same lock as a radio session. The connection menu reports failures and provides copyable diagnostics. A hardware power-on failure may still require a physical unplug/replug.

For a Windows source build, use a current Rust toolchain with Visual Studio C++ build tools/Windows SDK and `cargo build --release -p airbridge-app`. The repository's `Build.ps1` is an alternative used on the development machine, requiring its local `.tools/xwin-sdk` layout and Zig.

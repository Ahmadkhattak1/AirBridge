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

The startup UI checks connected hardware, USB sharing and runtime registration. Registration alone does not verify every runtime file or guarantee successful transfer; runtime startup reports subsequent errors.

For a Windows source build, use a current Rust toolchain with Visual Studio C++ build tools/Windows SDK and `cargo build --release -p airbridge-app`. The repository's `Build.ps1` is an alternative used on the development machine, requiring its local `.tools/xwin-sdk` layout and Zig.

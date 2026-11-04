# Install AirBridge 0.2.0

Download **AirBridge-0.2.0-Setup-x64.exe** from the GitHub release. Plug in a **Realtek RTL8188FTV USB adapter, ID `0bda:f179`**, then run Setup. Follow the administrator prompts for WSL and USB support. If Setup requests a restart, restart Windows and run Setup again.

This release supports x64 Windows 10 build 19041 or newer, including Windows 11, with hardware virtualization available. Setup requires internet access and about 2 GB of free space. Other Wi-Fi adapters are not supported by this release.

Setup installs the app for your Windows account and imports an Ubuntu-based `AirBridgeRuntime`. It downloads dependencies from their publishers and checks SHA-256 hashes. It installs WSL 2.7.13 and usbipd-win 5.3.0 if needed. The driver modules require `6.18.33.2-microsoft-standard-WSL2`. A newer, different kernel is not silently replaced. Existing distributions and WSL configuration are preserved.

Open AirBridge. The connection indicator at the top becomes **Ready**. Choose **Receive**, then share from the iPhone's built-in AirDrop menu and accept on Windows. Received files go to `Downloads\AirBridge`, unless you choose another folder. Sending requires Everyone for 10 Minutes on the unlocked iPhone.

The dedicated adapter stays attached to AirBridge while the app is open. First startup takes longer while WSL and the driver start. Later receive sessions reuse the connection. If the hardware fails to power on, unplug and reconnect the adapter. Connection diagnostics can be copied from the top menu.

## Repair and removal

Run Setup again with the adapter connected, or use **Repair AirBridge setup** in the Start menu. Setup logs are in `%LOCALAPPDATA%\AirBridge\setup.log`.

Uninstalling removes the Windows app and leaves received files, WSL, usbipd-win, and the runtime in place. To remove the dedicated runtime later, first back up any files inside it, close AirBridge, then run `wsl --unregister AirBridgeRuntime`. That command permanently deletes that distribution. WSL and usbipd-win can be removed separately using Windows settings if no other application needs them.

An existing manually provisioned development distribution without `/opt/airbridge/runtime-version` is preserved and blocks automatic migration. Keep using the development update instructions in `RUNTIME.md`, or back up that distribution before arranging a clean installation. Setup never unregisters a distribution.

## Building a release

1. Run the Windows and Linux CI checks at the intended release commit. Build the Windows application with `cargo build --release -p airbridge-app --locked` or `Build.ps1 -Release -AppOnly`.
2. In the matching provisioned Linux build environment, build `airbridge-cli` and `airbridge-radio`, then run `sudo bash scripts/package-runtime.sh OUTPUT_DIRECTORY`. The script copies an explicit allowlist of runtime files and excludes development files and private data.
3. Run `scripts/Build-Release.ps1 -RuntimeArchive PATH -Compiler PATH_TO_ISCC`. Use Inno Setup 6.7.1. The script embeds the runtime hash and resolved dependency license texts.
4. Import a fresh Ubuntu base into an isolated test distribution and test bootstrap, module loading, readiness, and TLS transfer validation. Test the GUI and hardware separately. Preserve existing runtimes during packaging tests.
5. Publish the installer, checksums, matching driver source archive, and Git backup bundle beside the release tag. The driver source archive includes the full upstream kernel tree, build configuration, symbol versions, and both AirBridge driver patches. Rust source is in the tagged repository.

The installer is not code-signed. Windows may show an unknown-publisher warning. Fresh-machine feature activation and a physical Windows reboot need testing on a separate clean Windows machine. An isolated WSL import on the development PC does not exercise those steps.

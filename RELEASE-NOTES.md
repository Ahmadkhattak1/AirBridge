# AirBridge 0.1.0 preview

Native Apple AirDrop for Windows — experimental developer preview.

## Included

- Windows desktop application with progressive send/receive controls and explicit incoming-file acceptance.
- Native iPhone-to-PC photo/video transfers and PC-to-iPhone transfers demonstrated on the development machine.
- Original HEIC/HEIF reception.
- Startup compatibility check, disabled transfer actions when prerequisites are missing, and a refresh action after connecting an adapter.
- Expandable, copyable compatibility report containing device model IDs, driver versions/provider and Windows version. No serial numbers, network addresses or file contents are included.
- Balanced signal-logo spacing.

## Before downloading

**The EXE is not a complete installer.** Transfers require an already provisioned `AirBridgeRuntime` WSL2 distribution, matching Linux kernel modules/firmware, and usbipd-win USB sharing. Those components are not bundled in this preview. A normal Windows Wi-Fi driver alone is insufficient. Without this setup the application can display compatibility information, but cannot transfer files.

Only the RTL8188FTV USB adapter with ID `0bda:f179` has been verified. Other adapters are not enabled in this release. The report helps contributors request support; it does not prove radio compatibility.

The receive service currently caps uploads/decoded archives at 512 MiB. Long transfers across the iPhone's ten-minute discovery expiry, wider iOS compatibility and interruption recovery are not validated. This preview has no Apple Account or Contacts Only integration.

## Existing development installations

Stop transfers and close the old application, then replace it with the provided `AirBridge.exe`. The runtime protocol is unchanged. Select **Check again** after plugging in the verified adapter. Keep the old executable if you want to roll back.

## Reporting an adapter

Open **Device details & compatibility → Copy compatibility report**. Paste it into a GitHub issue and add the brand, model and hardware revision printed on the adapter or packaging. Review the report before posting.

## Source and runtime

The repository includes Rust application/protocol sources, runtime orchestration, the experimental driver patch and research references. See `docs/RUNTIME.md` for prerequisites and the current packaging boundary.

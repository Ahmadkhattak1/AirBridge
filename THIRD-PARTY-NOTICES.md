# Third-party notices

AirBridge's Rust application code is developed in this repository and licensed under MIT.

The active radio uses the MIT-licensed `owly-awdl` and `owly-radio` libraries from https://github.com/t4t5/owly at pinned commit `9a676e11d011c34ea32798a2541f2b1eeb66e731`. The DNS and archive codecs use `owly-airdrop` from that same revision. AirBridge supplies its own Windows application, runtime orchestration, service announcements, TLS service and transfer controller. It does not run SwishDrop/SwooshDrop or the owly application.

The older Open Wireless Link (OWL) experiment at commit `0ee3263f944ea721c355f826aa90d636d012a512` is no longer used by the active runtime. Its GPLv3 source and license remain in `reference/owl-upstream` for research.

The test runtime also uses the Microsoft WSL2 Linux kernel, Ubuntu packages, the Linux `rtl8xxxu` driver, and Linux firmware under their respective upstream licenses. The locally built `rtl8xxxu.ko` corresponds to Microsoft WSL2 kernel tag `linux-msft-wsl-6.18.33.2`.

`reference/opendrop` is retained only as protocol research material and is not built or shipped as part of AirBridge.

The optional `rtl8xxxu-cck-experiment.ko` is a modified Linux GPL-2.0 driver, separate from the MIT application. Its change is supplied in `experiments/wifi_ble/rtl8188f-cck-experiment.patch`; matching full kernel sources remain in `/opt/airbridge/kernel` in the development runtime. The independently implemented CCK waveform generator is informed by the WiBeacon paper and Bluetooth sample vectors, with references and limitations in `experiments/wifi_ble/README.md`.

## owly license

MIT License

Copyright (c) 2026 T4T5

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

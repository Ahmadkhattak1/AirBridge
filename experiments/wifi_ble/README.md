# Wi-Fi CCK AirDrop wake experiment

This experiment was developed for the user's RTL8188FTV (0bda:f179) and iPhone 17 Pro Max. It is not a general Bluetooth adapter implementation.

## Method

`waveform.py` independently implements BLE framing, CRC and whitening, then chooses 11-Mbps CCK symbols whose filtered waveform approximates the advertisement on BLE channel 38 (2426 MHz). Wi-Fi uses channel 4 (2427 MHz). It checks the framing against the Bluetooth SIG sample vector and runs a software receiver model. `transmit.py` creates 128 DSSS scrambler-state variants and transmits them for a bounded interval.

`prepare_driver.py` and `rtl8188f-cck-experiment.patch` add an opt-in `airbridge_cck` parameter to the matching Linux rtl8xxxu driver. Only injected frames with the AirBridge experiment marker use the forced CCK rate and preserved sequence. The original module remains separately installed. The app enables the experiment for 20 seconds, disables it before AWDL, and reloads the original driver when the session ends.

The runtime embeds the generated 128 × 561-byte frame set in `crates/airbridge-radio/assets/cck-airdrop-ch38.bin`, so sending does not require Python, NumPy or SciPy. Those dependencies are only needed to regenerate/model the asset. `hardware_probe.sh` and `followup.py` preserve the original research procedure and logs.

## Evidence and limits

After the experiment the phone published an AirDrop receiver and completed a real text transfer. The user confirmed receipt. Evidence is in `output/wifi-ble-model/discovery-step-0-20261027T173136Z.send.log` and its accompanying events/radio logs. No separate RF sniffer measured the emitted BLE signal, and a controlled comparison remains outstanding. Do not equate a software model result or submitted packet count with verified reception.

## Research references

- [WiBeacon, MobiCom 2021](https://liux4189.github.io/files/WiBeacon_MobiCom_CameraReady.pdf): cross-technology Wi-Fi CCK waveform technique. The advertised repository did not provide the generator used here; this generator was implemented locally.
- [Bluetooth Core 5.4 sample data](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-54/out/en/low-energy-controller/sample-data.html): framing test vector.
- [Microsoft Sora CCK implementation](https://github.com/microsoft/Sora/blob/master/kernel/bb/Brick11/src/cck.hpp): checked CCK phase mapping; no Sora source copied.
- [Pinned AirDrop protocol notes](https://github.com/t4t5/owly/blob/9a676e11d011c34ea32798a2541f2b1eeb66e731/docs/SPEC.md): transfer metadata and connection sequence.

The Linux driver patch follows the upstream driver's GPL-2.0 license. The matching kernel source is retained at `/opt/airbridge/kernel` in the development runtime. The application and independently implemented waveform generator retain the project's MIT license.

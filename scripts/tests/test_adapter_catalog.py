import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
CATALOG = ROOT / "runtime/adapters.tsv"


class AdapterCatalog(unittest.TestCase):
    @unittest.skipIf(os.name == "nt", "Exercises Linux radio capability selection")
    def test_ht_profile_matches_selected_band_and_real_mcs_output(self):
        # RTL8188FTV iw output uses TX/RX, not RX. The old grep missed it.
        low = """Band 1:
            Capabilities: 0x6c
            Maximum RX AMPDU length 65535 bytes (exponent: 0x003)
            Minimum RX AMPDU time spacing: 16 usec (0x07)
            HT TX/RX MCS rate indexes supported: 0-7, 32
            Frequencies:
                * 2437 MHz [6] (20.0 dBm)
        """
        high = """Band 2:
            Capabilities: 0x16e
            Maximum RX AMPDU length 32767 bytes (exponent: 0x002)
            Minimum RX AMPDU time spacing: 4 usec (0x05)
            HT RX MCS rate indexes supported: 0-15
            HT TX MCS rate indexes are undefined
            Frequencies:
                * 5745 MHz [149] (23.0 dBm)
        """
        for report, channel, expected in [
            (low, 6, "006c 1f 000000ff"),
            (low + high, 6, "006c 1f 000000ff"),
            (low + high, 149, "016e 16 0000ffff"),
            (high + low, 149, "016e 16 0000ffff"),
            (low.replace("0-7, 32", "0-7, 8-15, 32"), 6, "006c 1f 0000ffff"),
            (low.replace("0-7, 32", "0-31"), 6, "006c 1f ffffffff"),
            (low, 149, None),
            (low.replace("Capabilities: 0x6c", "VHT Capabilities (0x6c):"), 6, None),
            (low.replace("0-7, 32", "none"), 6, None),
        ]:
            with self.subTest(channel=channel, report=report):
                run = subprocess.run(
                    ["bash", "-c", 'source "$1"; awdl_ht_profile "$2"', "ht-test",
                     str(ROOT / "runtime/adapter-profile.sh"), str(channel)],
                    input=report, capture_output=True, text=True, timeout=5,
                )
                self.assertEqual(run.returncode, int(expected is None), run.stderr)
                self.assertEqual(run.stdout.strip(), expected or "")

    @unittest.skipIf(os.name == "nt", "Exercises Linux radio channel selection")
    def test_selects_only_permitted_transfer_channels(self):
        low = "* 2437.0 MHz [6] (20.0 dBm)"
        high = "* 5745 MHz [149] (23.0 dBm)"
        for frequencies, expected in [
            ([low], "6"), ([low, high], "149"), ([high, low], "149"),
            ([low, high + " (no IR)"], "6"), ([low, high + " (radar detection)"], "6"),
            ([low, "* 5745 MHz [149] (disabled)"], "6"),
            ([low + " (no IR)", "* 5180 MHz [36] (23.0 dBm)"], None),
        ]:
            with self.subTest(frequencies=frequencies):
                run = subprocess.run(
                    ["bash", "-c", 'source "$1"; awdl_channel', "channel-test",
                     str(ROOT / "runtime/adapter-profile.sh")],
                    input="\n".join(frequencies), capture_output=True, text=True, timeout=5,
                )
                self.assertEqual(run.returncode, int(expected is None))
                self.assertEqual(run.stdout.strip(), expected or "")

    def test_profiles_are_unique_and_use_packaged_firmware(self):
        ids = set()
        chips = set()
        for line in CATALOG.read_text().splitlines():
            if not line or line.startswith("#"):
                continue
            usb_id, chipset, driver, firmware, rate = line.split("\t")
            self.assertRegex(usb_id, r"^[0-9a-f]{4}:[0-9a-f]{4}$")
            self.assertNotIn(usb_id, ids)
            ids.add(usb_id)
            chips.add(chipset)
            self.assertEqual(driver, "rtl8xxxu")
            self.assertRegex(firmware, r"^rtlwifi/[a-zA-Z0-9_]+\.bin$")
            self.assertEqual(rate, "mcs7")
        self.assertGreaterEqual(len(chips), 4)
        self.assertIn("0bda:f179", ids)

    @unittest.skipIf(os.name == "nt", "Exercises Linux USB sysfs selection")
    def test_selects_each_profile_and_refuses_multiple_or_unknown_devices(self):
        rows = [line.split("\t") for line in CATALOG.read_text().splitlines()
                if line and not line.startswith("#")]
        with tempfile.TemporaryDirectory() as directory:
            devices = pathlib.Path(directory)
            radio = devices / "1-2"
            radio.mkdir()
            (radio / "idVendor").write_text("0bda\n")
            (radio / "idProduct").write_text("ffff\n")

            def select():
                return subprocess.run([
                    "bash", "-c",
                    'source "$1"; select_adapter "$2" "$3" || exit $?; '
                    'printf "%s|%s|%s|%s\\n" "$selected_usb" "$usb_id" "$chipset" "$firmware"',
                    "adapter-test", str(ROOT / "runtime/adapter-profile.sh"),
                    str(CATALOG), str(devices),
                ], capture_output=True, text=True, timeout=5)

            self.assertEqual(select().returncode, 1)
            for usb_id, chipset, _, firmware, _ in rows:
                with self.subTest(usb_id=usb_id):
                    vendor, product = usb_id.split(":")
                    (radio / "idVendor").write_text(vendor + "\n")
                    (radio / "idProduct").write_text(product + "\n")
                    result = select()
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.strip(), f"{radio}|{usb_id}|{chipset}|{firmware}")
            extra = devices / "1-3"
            extra.mkdir()
            (extra / "idVendor").write_text("0bda\n")
            (extra / "idProduct").write_text("f179\n")
            result = select()
            self.assertEqual(result.returncode, 2)
            self.assertIn("More than one", result.stderr)

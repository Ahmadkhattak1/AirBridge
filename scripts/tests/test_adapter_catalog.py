import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
CATALOG = ROOT / "runtime/adapters.tsv"


class AdapterCatalog(unittest.TestCase):
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

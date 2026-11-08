import json
import pathlib
import re
import unittest
import tempfile
import hashlib
import os
import subprocess
import struct

ROOT = pathlib.Path(__file__).resolve().parents[2]


class ReleaseContract(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Checks the Windows release executable")
    def test_windows_release_has_no_external_visual_cpp_runtime_dependency(self):
        binary = (ROOT / "target/release/airbridge-ui.exe").read_bytes()
        pe = struct.unpack_from("<I", binary, 0x3C)[0]
        self.assertEqual(binary[pe:pe + 4], b"PE\0\0")
        sections = struct.unpack_from("<H", binary, pe + 6)[0]
        optional_size = struct.unpack_from("<H", binary, pe + 20)[0]
        optional = pe + 24
        self.assertEqual(struct.unpack_from("<H", binary, optional)[0], 0x20B)

        def offset(rva):
            for index in range(sections):
                section = optional + optional_size + index * 40
                virtual_size, address, size, raw = struct.unpack_from("<IIII", binary, section + 8)
                if address <= rva < address + max(virtual_size, size):
                    return raw + rva - address
            self.fail(f"Invalid executable RVA {rva}")

        imports_rva = struct.unpack_from("<I", binary, optional + 120)[0]
        cursor = offset(imports_rva)
        dependencies = []
        while any(binary[cursor:cursor + 20]):
            name_rva = struct.unpack_from("<I", binary, cursor + 12)[0]
            start = offset(name_rva)
            dependencies.append(binary[start:binary.index(b"\0", start)].decode().lower())
            cursor += 20
        self.assertTrue(dependencies)
        self.assertFalse([name for name in dependencies if name.startswith(("vcruntime", "msvcp"))], dependencies)

    @unittest.skipUnless(os.name == "nt", "Runs the real Windows setup verifier")
    def test_real_setup_verifier_accepts_valid_payload_and_rejects_corruption(self):
        for corrupt in (False, True):
            with self.subTest(corrupt=corrupt), tempfile.TemporaryDirectory() as directory:
                payload = pathlib.Path(directory)
                archive = b"isolated validation fixture"
                (payload / "airbridge-runtime.tar.gz").write_bytes(archive)
                dependencies = json.loads((ROOT / "installer/dependencies.json").read_text())
                (payload / "dependencies.json").write_text(json.dumps(dependencies))
                (payload / "runtime-manifest.json").write_text(json.dumps({
                    "version": "0.3.0", "kernel": dependencies["kernel"],
                    "sha256": "0" * 64 if corrupt else hashlib.sha256(archive).hexdigest(),
                }))
                result = subprocess.run([
                    "powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", str(ROOT / "installer/Setup-Runtime.ps1"),
                    "-InstallRoot", directory, "-ValidateOnly",
                ], env={**os.environ, "LOCALAPPDATA": directory}, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 1 if corrupt else 0, result.stdout + result.stderr)
                if not corrupt:
                    self.assertIn("PACKAGE_VALIDATED", result.stdout)

    def test_downloads_have_fixed_https_urls_and_sha256(self):
        manifest = json.loads((ROOT / "installer/dependencies.json").read_text())
        for name in ("ubuntu", "wsl", "usbipd"):
            dependency = manifest[name]
            self.assertTrue(dependency["url"].startswith("https://"))
            self.assertRegex(dependency["sha256"], r"^[a-f0-9]{64}$")
            self.assertRegex(dependency["file"], r"^[a-zA-Z0-9.-]+$")

    def test_package_does_not_export_developer_data(self):
        script = (ROOT / "scripts/package-runtime.sh").read_text()
        self.assertNotIn("wsl --export", script)
        self.assertNotRegex(script, r"cp\s+.*(/root|/home|/var/log)")
        self.assertIn("rtl8xxxu-awdl", script)
        self.assertIn("rtl8xxxu-cck-experiment", script)

    def test_installer_verifies_payload_before_mutating_runtime(self):
        script = (ROOT / "installer/Setup-Runtime.ps1").read_text()
        self.assertLess(script.index("Assert-Hash $overlay"), script.index("'--import'"))
        self.assertIn("if ($ValidateOnly)", script)
        self.assertIn("An unmanaged AirBridgeRuntime already exists", script)
        self.assertNotIn("--unregister", script)
        self.assertNotIn(".wslconfig", script)

    def test_ci_runs_both_platforms_and_regression_guard(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("ubuntu-24.04, windows-latest", workflow)
        self.assertIn("test --workspace --locked", workflow)
        self.assertIn("check-test-contract.py", workflow)
        self.assertIn("contents: read", workflow)


if __name__ == "__main__":
    unittest.main()

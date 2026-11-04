import json
import pathlib
import re
import unittest
import tempfile
import hashlib
import os
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]


class ReleaseContract(unittest.TestCase):
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
                    "version": "0.2.0", "kernel": dependencies["kernel"],
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

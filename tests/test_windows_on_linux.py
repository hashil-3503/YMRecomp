"""Game-free checks that YM's Windows files are made on the Linux runner (docs/dev/reference/ci.md: there is no Windows
one): no workflow asks for a Windows runner, which would queue forever and hold a release; the Linux llvm-mingw that compiles
them is tools.lock's release; the Windows YM-builder's job checks what it made and the bundle's dav1d (no dav1d is
built in CI); and a tag whose commit CI released already skips only the checks that ship no file.

Run with: python -I tests/test_windows_on_linux.py
"""
import importlib.util
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".forgejo" / "workflows"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


class WindowsOnLinuxTests(unittest.TestCase):
    def test_no_windows_runner(self):
        for wf in sorted(WORKFLOWS.glob("*.yml")):
            text = wf.read_text(encoding="utf-8")
            with self.subTest(wf.name):
                self.assertIsNone(re.search(r"runs-on:.*windows", text, re.I))
                # (a matrix's os, which runs-on names through ${{ matrix.os }})
                self.assertIsNone(re.search(r"^\s*-?\s*os:\s*windows", text, re.I | re.M))

    def test_linux_llvm_mingw_is_tools_lock_release(self):
        package = module("YM_build_under_test", ROOT / "scripts" / "YM_builder.py")
        lock = (ROOT / "tools.lock").read_text(encoding="utf-8").splitlines()
        windows = [l.split()[2] for l in lock if l.startswith("tool llvm-mingw ") and l.split()[3] == "windows-x64"]
        version, prefix, sha256, url = package.linux_llvm_mingw()
        self.assertEqual(len(windows), 1)
        self.assertTrue(windows[0].startswith(version + "-"), (windows, version))
        self.assertEqual(prefix, "llvm-mingw-%s-ucrt-ubuntu-22.04-x86_64" % version)
        self.assertRegex(sha256, r"^[0-9a-f]{64}$")
        self.assertEqual(url, "https://github.com/mstorsjo/llvm-mingw/releases/download/%s/%s.tar.xz" % (version, prefix))

if __name__ == "__main__":
    unittest.main()

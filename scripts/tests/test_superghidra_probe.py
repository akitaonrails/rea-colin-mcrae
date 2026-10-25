from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path, PurePosixPath
from types import SimpleNamespace


SCRIPT = Path(__file__).resolve().parents[1] / "superghidra_probe.py"
SPEC = importlib.util.spec_from_file_location("superghidra_probe", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
PROBE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = PROBE
SPEC.loader.exec_module(PROBE)


class FakeClassReport:
    def to_dict(self):
        return {"counts": {"rtti": 2, "mfc": 1, "mfc_serializable": 1}}


def fake_api():
    section = SimpleNamespace(
        name=".text",
        vaddr=0x401000,
        vsize=0x80,
        file_size=0x80,
        readable=True,
        writable=False,
        executable=True,
    )
    image = SimpleNamespace(
        format="pe",
        arch="x86",
        bits=32,
        image_base=0x400000,
        entry=0x401000,
        sections=[section],
        libraries=["KERNEL32.dll", "USER32.dll", "kernel32.DLL"],
        imports=[
            SimpleNamespace(library="KERNEL32.dll", name="ExitProcess", ordinal=None),
            SimpleNamespace(library="USER32.dll", name=None, ordinal=7),
        ],
    )
    return PROBE.SuperGhidraApi(
        version="test",
        commit="a" * 40,
        load_binary=lambda _path: image,
        recover_classes=lambda _path: FakeClassReport(),
        extract_cfgs_from_binary=lambda _path: [
            SimpleNamespace(address=0x401020),
            SimpleNamespace(address=0x401000),
            SimpleNamespace(address=0x401020),
        ],
    )


class SuperGhidraProbeTests(unittest.TestCase):
    def test_source_relative_path_rejects_escape_and_absolute_forms(self):
        for value in ("../Rally.exe", "/games/Rally.exe", "C:/games/Rally.exe", "bin\\Rally.exe"):
            with self.subTest(value=value), self.assertRaises(PROBE.ProbeError):
                PROBE.source_relative_path(value)
        self.assertEqual(PROBE.source_relative_path("bin/Rally.exe").as_posix(), "bin/Rally.exe")

    def test_resolved_symlink_cannot_escape_source_root(self):
        with tempfile.TemporaryDirectory() as root_name, tempfile.TemporaryDirectory() as other_name:
            root, other = Path(root_name), Path(other_name)
            target = other / "Rally.exe"
            target.write_bytes(b"outside")
            (root / "Rally.exe").symlink_to(target)
            with self.assertRaises(PROBE.ProbeError):
                PROBE.resolve_source_file(root, PurePosixPath("Rally.exe"))

    def test_report_is_digest_bound_deterministic_and_path_sanitized(self):
        with tempfile.TemporaryDirectory() as root_name:
            root = Path(root_name)
            executable = root / "Rally.exe"
            executable.write_bytes(b"synthetic executable shape")
            first, starts = PROBE.build_report(executable, PurePosixPath("Rally.exe"), fake_api())
            second, _ = PROBE.build_report(executable, PurePosixPath("Rally.exe"), fake_api())
            self.assertEqual(first, second)
            self.assertEqual(starts, {0x401000, 0x401020})
            self.assertEqual(first["artifact"]["path"], "Rally.exe")
            self.assertNotIn(str(root), json.dumps(first))
            self.assertRegex(first["artifact"]["sha256"], r"^[0-9a-f]{64}$")
            self.assertEqual(first["observations"]["libraries"], ["kernel32.dll", "user32.dll"])

    def test_seed_output_is_sorted_deduplicated_and_strict(self):
        candidates = {0x401000, 0x401020}
        self.assertEqual(
            PROBE.rea_seed_text([0x401020, 0x401000, 0x401020], candidates),
            "0x401000\tfunction\n0x401020\tfunction\n",
        )
        with self.assertRaises(PROBE.ProbeError):
            PROBE.rea_seed_text([0xDEADBEEF], candidates)

    def test_output_must_stay_in_work_and_seed_file_is_optional(self):
        with tempfile.TemporaryDirectory() as root_name:
            repository = Path(root_name)
            (repository / ".work").mkdir()
            allowed = PROBE.resolve_output_dir(repository / ".work" / "superghidra", repository)
            self.assertEqual(allowed, (repository / ".work" / "superghidra").resolve())
            with self.assertRaises(PROBE.ProbeError):
                PROBE.resolve_output_dir(repository / "docs", repository)

            report_path, seed_path = PROBE.write_outputs(allowed, {"schema": PROBE.SCHEMA}, "")
            self.assertTrue(report_path.is_file())
            self.assertIsNone(seed_path)
            report_path, seed_path = PROBE.write_outputs(allowed, {"schema": PROBE.SCHEMA}, "0x1\tfunction\n")
            self.assertTrue(report_path.is_file())
            self.assertEqual(seed_path.read_text(), "0x1\tfunction\n")
            report_path, seed_path = PROBE.write_outputs(allowed, {"schema": PROBE.SCHEMA}, "")
            self.assertTrue(report_path.is_file())
            self.assertIsNone(seed_path)
            self.assertFalse((allowed / "rea-seeds.tsv").exists())


if __name__ == "__main__":
    unittest.main()

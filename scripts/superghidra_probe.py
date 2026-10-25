#!/usr/bin/env python3
"""Create a sanitized, ignored SuperGhidra cross-check report for REA.

SuperGhidra remains an optional external analysis tool.  This wrapper imports it
from an explicit checkout, analyzes one executable below an explicit source
root, and writes only metadata to ``.work/superghidra``.  It never emits target
bytes or pseudocode.  Selected, already-reviewed CFG starts can also be emitted
in REA's strict seed TSV format.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Iterable


SCHEMA = "cmr-superghidra-crosscheck-v1"
HEX_DIGEST = re.compile(r"[0-9a-f]{64}\Z")
GIT_COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class ProbeError(ValueError):
    """The requested probe would produce ambiguous or unsafe evidence."""


@dataclass(frozen=True)
class SuperGhidraApi:
    version: str
    commit: str | None
    load_binary: Callable[[str], Any]
    recover_classes: Callable[[str], Any]
    extract_cfgs_from_binary: Callable[[str], list[Any]]


def source_relative_path(value: str) -> PurePosixPath:
    """Return a portable path that cannot escape the selected source root."""
    if "\\" in value:
        raise ProbeError("executable path must use portable '/' separators")
    path = PurePosixPath(value)
    if path.is_absolute() or not path.parts or any(part in ("", ".", "..") for part in path.parts):
        raise ProbeError("executable must be a non-empty source-relative path")
    if ":" in path.parts[0]:
        raise ProbeError("executable must not contain a drive prefix")
    return path


def resolve_source_file(source: Path, relative: PurePosixPath) -> Path:
    root = source.resolve(strict=True)
    candidate = root.joinpath(*relative.parts).resolve(strict=True)
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise ProbeError("executable resolves outside the selected source root") from exc
    if not candidate.is_file():
        raise ProbeError("selected executable is not a regular file")
    return candidate


def resolve_output_dir(value: Path, repository: Path) -> Path:
    work = (repository / ".work").resolve()
    output = value.resolve()
    try:
        output.relative_to(work)
    except ValueError as exc:
        raise ProbeError("probe output must remain below this repository's .work directory") from exc
    return output


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checkout_commit(root: Path) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
    )
    value = result.stdout.strip().lower()
    return value if result.returncode == 0 and GIT_COMMIT.fullmatch(value) else None


def load_superghidra(root: Path) -> SuperGhidraApi:
    checkout = root.resolve(strict=True)
    if not (checkout / "superghidra" / "__init__.py").is_file():
        raise ProbeError("--superghidra-root is not a SuperGhidra source checkout")
    sys.path.insert(0, str(checkout))
    try:
        from superghidra import __version__
        from superghidra.diff.binary_diff_engine import extract_cfgs_from_binary
        from superghidra.formats.binary_loader import load_binary
        from superghidra.formats.msvc_class_recovery import recover_classes
    except ImportError as exc:
        raise ProbeError(f"cannot import SuperGhidra from the selected checkout: {exc}") from exc
    return SuperGhidraApi(
        version=str(__version__),
        commit=checkout_commit(checkout),
        load_binary=load_binary,
        recover_classes=recover_classes,
        extract_cfgs_from_binary=extract_cfgs_from_binary,
    )


def _section_dict(section: Any) -> dict[str, Any]:
    return {
        "name": str(section.name),
        "virtual_address": f"0x{int(section.vaddr):x}",
        "virtual_size": int(section.vsize),
        "file_size": int(section.file_size),
        "readable": bool(section.readable),
        "writable": bool(section.writable),
        "executable": bool(section.executable),
    }


def build_report(
    executable: Path,
    relative: PurePosixPath,
    api: SuperGhidraApi,
) -> tuple[dict[str, Any], set[int]]:
    digest = sha256_file(executable)
    if not HEX_DIGEST.fullmatch(digest):
        raise ProbeError("internal SHA-256 formatting failure")

    image = api.load_binary(str(executable))
    if image.format != "pe" or image.arch not in ("x86", "x86_64"):
        raise ProbeError(f"expected an x86/x86_64 PE image, got {image.format}/{image.arch}")

    classes = api.recover_classes(str(executable)).to_dict()
    cfgs = api.extract_cfgs_from_binary(str(executable))
    starts = {int(cfg.address) for cfg in cfgs}
    libraries = sorted({str(value).lower() for value in image.libraries})
    imports = sorted(
        {
            (str(item.library).lower(), str(item.name) if item.name is not None else None, item.ordinal)
            for item in image.imports
        },
        key=lambda item: (item[0], item[1] or "", item[2] if item[2] is not None else -1),
    )
    report = {
        "schema": SCHEMA,
        "artifact": {
            "path": relative.as_posix(),
            "size": executable.stat().st_size,
            "sha256": digest,
        },
        "tool": {
            "name": "SuperGhidra",
            "version": api.version,
            "commit": api.commit,
        },
        "observations": {
            "format": image.format,
            "architecture": image.arch,
            "bits": int(image.bits),
            "image_base": f"0x{int(image.image_base):x}",
            "entry": f"0x{int(image.entry):x}",
            "sections": [_section_dict(section) for section in image.sections],
            "libraries": libraries,
            "imports": [
                {"library": library, "name": name, "ordinal": ordinal}
                for library, name, ordinal in imports
            ],
            "rtti_class_count": int(classes["counts"]["rtti"]),
            "mfc_class_count": int(classes["counts"]["mfc"]),
            "mfc_serializable_count": int(classes["counts"]["mfc_serializable"]),
            "cfg_candidate_count": len(starts),
            "cfg_candidate_addresses": [f"0x{address:x}" for address in sorted(starts)],
        },
        "limitations": [
            "Static analysis only; the executable was not run.",
            "CFG starts are SuperGhidra candidates, not established function boundaries.",
            "RTTI and MFC scans are bounded to SuperGhidra's supported PE patterns.",
            "REA must corroborate a selected candidate before it supports a compatibility claim.",
        ],
    }
    return report, starts


def parse_addresses(values: Iterable[str]) -> list[int]:
    parsed: set[int] = set()
    for value in values:
        try:
            address = int(value, 0)
        except ValueError as exc:
            raise ProbeError(f"invalid seed address: {value}") from exc
        if address < 0 or address > 0xFFFFFFFFFFFFFFFF:
            raise ProbeError(f"seed address is outside the supported range: {value}")
        parsed.add(address)
    return sorted(parsed)


def rea_seed_text(selected: Iterable[int], candidates: set[int]) -> str:
    rows = []
    for address in sorted(set(selected)):
        if address not in candidates:
            raise ProbeError(f"0x{address:x} is not a SuperGhidra CFG candidate")
        rows.append(f"0x{address:x}\tfunction")
    return "".join(f"{row}\n" for row in rows)


def write_outputs(output: Path, report: dict[str, Any], seeds: str) -> tuple[Path, Path | None]:
    output.mkdir(parents=True, exist_ok=True)
    report_path = output / "crosscheck.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    seed_path = None
    if seeds:
        seed_path = output / "rea-seeds.tsv"
        seed_path.write_text(seeds, encoding="utf-8")
    else:
        stale_seed_path = output / "rea-seeds.tsv"
        if stale_seed_path.exists():
            stale_seed_path.unlink()
    return report_path, seed_path


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="Original-media root")
    parser.add_argument(
        "--executable", default="Rally.exe", help="Portable path below --source (default: Rally.exe)"
    )
    parser.add_argument("--superghidra-root", type=Path, required=True, help="SuperGhidra checkout")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(".work/superghidra"),
        help="Ignored output below this repository's .work directory",
    )
    parser.add_argument(
        "--seed-address",
        action="append",
        default=[],
        help="Reviewed SuperGhidra CFG start to emit for REA (repeatable)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = create_parser().parse_args(argv)
    repository = Path(__file__).resolve().parent.parent
    try:
        relative = source_relative_path(args.executable)
        executable = resolve_source_file(args.source, relative)
        output = resolve_output_dir(args.output_dir, repository)
        api = load_superghidra(args.superghidra_root)
        report, candidates = build_report(executable, relative, api)
        selected = parse_addresses(args.seed_address)
        report_path, seed_path = write_outputs(output, report, rea_seed_text(selected, candidates))
    except (OSError, ProbeError, KeyError, TypeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    observations = report["observations"]
    print(
        f"SuperGhidra cross-check: {report['artifact']['sha256']} "
        f"({observations['architecture']}, {observations['cfg_candidate_count']} CFG candidates)"
    )
    print(f"Sanitized report: {report_path.relative_to(repository)}")
    if seed_path is not None:
        print(f"REA seeds: {seed_path.relative_to(repository)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

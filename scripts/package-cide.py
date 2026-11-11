#!/usr/bin/env python3
"""Opt-in local development ZIP. No installation, publication or system changes."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import zipfile

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

CHUNK = 64 * 1024
MAX_PAYLOAD = 512 * 1024 * 1024
MAX_NOTICE = 512 * 1024
MAX_NOTICE_TOTAL = 32 * 1024 * 1024
ALIASES = {"cide": "jc-ide", "cide-tui": "jc-ide-tui", "cide-gui": "jc-ide-gui"}
BINARIES = [name + ".exe" for pair in ALIASES.items() for name in pair]
FIXED = [
    "assets/branding/cide-tui.ico", "assets/branding/cide-gui.ico",
    "assets/branding/cide-tui-256.png", "assets/branding/cide-gui-256.png",
    "assets/branding/provenance.json", "README.md", "LICENSE", "LICENSING.md",
    "docs/GIT_EMBEDDED_ENGINE.md", "docs/GIT_EMBEDDED_LICENSES.md",
    "docs/GIT_CAPABILITIES_CIDE_AUDIT.md",
    "docs/GIT_UX_REA_20261110.md",
    "docs/M2_SIDEBAR_NAVIGATION.md",
    "docs/M2_SELECTION_HISTORY.md",
    "docs/M2_GIT_WORKSPACE_NATIVE.md",
    "docs/G1_HISTORICAL_READS.md",
    "docs/G1_LOCAL_SOURCE_PAIRS.md",
    "docs/G2_GIT_LAYOUT_COMPARISON.md",
    "docs/G2_GIT_VISUAL_REDESIGN.md",
    "docs/G3_PATH_HISTORY_OUTLINE.md",
    "docs/G3_BLAME.md",
    "docs/GIT_SHARED_CORE.md",
    "third_party/limetext-text-LICENSE", "third_party/unicode-17/LICENSE.txt",
]
DEVELOPMENT = """CIDE 0.1.0 - Windows MSVC x64 development package

Run bin/cide-gui.exe for the native GUI, bin/cide-tui.exe for the TUI,
or bin/cide.exe --help for the headless CLI.
Legacy aliases: jc-ide-gui.exe, jc-ide-tui.exe and jc-ide.exe respectively.
Canonical and legacy commands use the same implementation.
The GUI and TUI use the embedded Rust Git engine for status, history, local
writes and HTTPS/SSH operations. See docs/GIT_CAPABILITIES_CIDE_AUDIT.md for
validated workflows and scope. The editor does not require git.exe.

Use --encoding utf8 explicitly for UTF-8 source when appropriate. Legacy
encodings are explicit choices; the owner's actual legacy page is unconfirmed.
This package does not certify manual IME, physical DPI, accessibility, global
clipboard interoperability or physical-display latency/RAM acceptance.

This is a local development archive, not an installer or a release/legal review.
No shortcuts, registry keys, services or global configuration are installed.
LICENSE and LICENSING.md are copied from the repository without reinterpretation.
Review them and manifest.json before any redistribution. Registry notices are a
conservative Cargo.lock/offline-cache inventory, not proof of the exact Windows
linked dependency graph. Missing/skipped notices are explicit in the manifest.
"""


class PackagingError(Exception):
    pass


def ordinary(metadata: os.stat_result) -> bool:
    return not stat.S_ISLNK(metadata.st_mode) and not (
        getattr(metadata, "st_file_attributes", 0) & 0x400
    )


def checked(path: Path, workspace: Path, *, directory: bool = False) -> Path:
    if ".." in path.parts or not path.is_absolute() or not path.is_relative_to(workspace):
        raise PackagingError("path is outside the workspace")
    # Check each component, including links that resolve back inside the root.
    relative = path.relative_to(workspace)
    current = workspace
    if not ordinary(current.lstat()):
        raise PackagingError("workspace is not ordinary")
    for part in relative.parts:
        current /= part
        if not ordinary(current.lstat()):
            raise PackagingError("links/reparse points are not package inputs")
    if directory:
        if not current.is_dir():
            raise PackagingError("expected input directory")
    elif not current.is_file():
        raise PackagingError("expected input file")
    if not current.resolve(strict=True).is_relative_to(workspace):
        raise PackagingError("resolved path is outside the workspace")
    return current


def notice_sources(workspace: Path, preserved: set[str]) -> tuple[list[tuple[Path, str]], list[dict]]:
    if tomllib is None:
        # Do not silently omit notices on old Python; no output is created.
        raise PackagingError("Python 3.11+ with tomllib is required for offline notice inventory")
    lock = checked(workspace / "Cargo.lock", workspace)
    if lock.stat().st_size > 2 * 1024 * 1024:
        raise PackagingError("Cargo.lock exceeds inventory budget")
    with lock.open("rb") as stream:
        packages = tomllib.load(stream).get("package", [])
    if not isinstance(packages, list) or len(packages) > 512:
        raise PackagingError("unexpected Cargo.lock package inventory")
    roots = []
    for relative in ("work/toolchain/cargo/registry/src", ".cargo/registry/src"):
        candidate = workspace / relative
        if candidate.exists():
            roots.append(checked(candidate, workspace, directory=True))
    indexes = []
    for root in roots:
        for candidate in sorted(root.iterdir()):
            try:
                indexes.append(checked(candidate, workspace, directory=True))
            except (OSError, PackagingError):
                continue
    notices = []
    missing = []
    total_bytes = 0
    seen = set()
    for package in packages:
        source = package.get("source", "")
        if not isinstance(source, str) or not source.startswith("registry+"):
            continue
        name, version = package.get("name", ""), package.get("version", "")
        if not isinstance(name, str) or not isinstance(version, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not re.fullmatch(r"[A-Za-z0-9.+-]+", version):
            raise PackagingError("invalid registry package identity")
        identity = name + "-" + version
        if identity in preserved:
            continue  # Exact notices already included from the checked Git inventory.
        if identity in seen:
            continue
        seen.add(identity)
        package_root = None
        for index in indexes:
            candidate = index / identity
            if candidate.exists():
                try:
                    package_root = checked(candidate, workspace, directory=True)
                    break
                except (OSError, PackagingError):
                    continue
        if package_root is None:
            missing.append({"package": identity, "reason": "offline source not present"})
            continue
        found = 0
        skipped = False
        visited = 0
        traversal_errors = []
        # Never follow links and never copy Cargo sources/manifests. Scan only
        # this exact lockfile package; notices may live in a nested licenses dir.
        for directory, dirs, files in os.walk(package_root, followlinks=False, onerror=lambda error: traversal_errors.append(True)):
            visited += len(dirs) + len(files)
            if visited > 20_000:
                skipped = True
                break
            safe_dirs = []
            for child in sorted(dirs):
                try:
                    checked(Path(directory) / child, workspace, directory=True)
                    safe_dirs.append(child)
                except (OSError, PackagingError):
                    skipped = True
            dirs[:] = safe_dirs
            for filename in sorted(files):
                if not filename.upper().startswith(("LICENSE", "NOTICE", "COPYRIGHT")):
                    continue
                try:
                    path = checked(Path(directory) / filename, workspace)
                    size = path.stat().st_size
                    if size > MAX_NOTICE or total_bytes + size > MAX_NOTICE_TOTAL or found >= 128 or len(notices) >= 1024:
                        skipped = True
                        continue
                except (OSError, PackagingError):
                    skipped = True
                    continue
                archive = "third_party/registry/" + identity + "/" + path.relative_to(package_root).as_posix()
                notices.append((path, archive))
                total_bytes += size
                found += 1
        skipped = skipped or bool(traversal_errors)
        if not found or skipped:
            missing.append({"package": identity, "reason": "some matching notices skipped or unavailable" if skipped else "no matching notices in offline source"})
    return notices, missing


def copy_member(archive: zipfile.ZipFile, source: Path, name: str, workspace: Path) -> dict:
    source = checked(source, workspace)
    before = source.stat()
    limit = MAX_NOTICE if name.startswith("third_party/") else MAX_PAYLOAD
    if before.st_size > limit:
        raise PackagingError("payload exceeds per-file size budget")
    digest = hashlib.sha256()
    length = 0
    with source.open("rb") as incoming, archive.open(name, "w", force_zip64=True) as outgoing:
        while chunk := incoming.read(CHUNK):
            length += len(chunk)
            if length > limit:
                raise PackagingError("payload grew beyond size budget")
            digest.update(chunk)
            outgoing.write(chunk)
    after = source.stat()
    if (before.st_size, before.st_mtime_ns, before.st_ino) != (after.st_size, after.st_mtime_ns, after.st_ino) or length != before.st_size:
        raise PackagingError("source changed during packaging")
    return {"path": name, "bytes": length, "sha256": digest.hexdigest()}


def embedded_git_notices(workspace: Path) -> tuple[list[tuple[Path, str]], set[str]]:
    root = checked(workspace / "third_party/gitoxide", workspace, directory=True)
    inventory_path = checked(root / "DEPENDENCIES.json", workspace)
    if inventory_path.stat().st_size > MAX_NOTICE:
        raise PackagingError("embedded Git inventory exceeds budget")
    inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
    if inventory.get("cargo_lock_sha256") != hashlib.sha256((workspace / "Cargo.lock").read_bytes()).hexdigest():
        raise PackagingError("embedded Git notices do not match Cargo.lock")
    preserved = set()
    for package in inventory["packages"]:
        identity = package["name"] + "-" + package["version"]
        if not re.fullmatch(r"[A-Za-z0-9_.+-]+", identity) or not package["notices"]:
            raise PackagingError("invalid embedded Git license identity")
        for notice in package["notices"]:
            path = checked(root / "dependencies" / identity / notice["path"], workspace)
            if path.stat().st_size > MAX_NOTICE or path.stat().st_size != notice["bytes"] or hashlib.sha256(path.read_bytes()).hexdigest() != notice["sha256"]:
                raise PackagingError("embedded Git license differs from inventory")
        preserved.add(identity)
    result = []
    total = 0
    for path in sorted(root.rglob("*")):
        if path.is_dir():
            checked(path, workspace, directory=True)
            continue
        path = checked(path, workspace)
        total += path.stat().st_size
        if len(result) >= 1024 or path.stat().st_size > MAX_NOTICE or total > MAX_NOTICE_TOTAL:
            raise PackagingError("embedded Git notices exceed budget")
        result.append((path, path.relative_to(workspace).as_posix()))
    return result, preserved


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("work/packages/CIDE-windows-dev.zip"))
    parser.add_argument("--bin-dir", type=Path, default=Path("target/release"), help="built binaries inside this workspace")
    args = parser.parse_args()
    workspace = Path(__file__).resolve().parents[1]
    output = args.output if args.output.is_absolute() else workspace / args.output
    created = False
    owned_identity = None
    try:
        if os.name != "nt":
            raise PackagingError("Windows development packaging requires Windows")
        checked(output.parent, workspace, directory=True)
        if output.name in ("", ".", "..") or os.path.lexists(output):
            raise PackagingError("output must be a new workspace file")
        # Parent is checked before creation; exclusive open refuses existing
        # files, including dangling links. No directories are created here.
        binary_directory = args.bin_dir if args.bin_dir.is_absolute() else workspace / args.bin_dir
        checked(binary_directory, workspace, directory=True)
        payload = [(checked(binary_directory / name, workspace), "bin/" + name) for name in BINARIES]
        payload += [(checked(workspace / relative, workspace), relative) for relative in FIXED]
        embedded, preserved = embedded_git_notices(workspace)
        payload += embedded
        notices, missing = notice_sources(workspace, preserved)
        payload += notices
        if len({name for _, name in payload}) != len(payload):
            raise PackagingError("duplicate archive member")
        with output.open("xb") as file:
            created = True
            metadata = os.fstat(file.fileno())
            owned_identity = (metadata.st_dev, metadata.st_ino)
            with zipfile.ZipFile(file, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
                members = []
                notice_bytes = 0
                for path, name in payload:
                    member = copy_member(archive, path, name, workspace)
                    if name.startswith("third_party/registry/"):
                        notice_bytes += member["bytes"]
                        if notice_bytes > MAX_NOTICE_TOTAL:
                            raise PackagingError("registry notices grew beyond total budget")
                    members.append(member)
                development = DEVELOPMENT.encode("utf-8")
                archive.writestr("DEVELOPMENT.txt", development)
                members.append({"path": "DEVELOPMENT.txt", "bytes": len(development), "sha256": hashlib.sha256(development).hexdigest()})
                manifest = {
                    "product": "CIDE", "version": "0.1.0",
                    "platform": "Windows MSVC x64", "channel": "development",
                    "canonical_to_legacy": ALIASES,
                    "files": members,
                    "hash_scope": "all payload files except manifest.json itself",
                    "notices_scope": "registry packages from Cargo.lock present in workspace offline cache; conservative superset, exact Windows linked graph not established",
                    "missing_notices": missing,
                    "licensing_review": "See included LICENSE and LICENSING.md; this archive is not legal/release certification",
                }
                archive.writestr("manifest.json", json.dumps(manifest, ensure_ascii=True, indent=2, sort_keys=True).encode("utf-8"))
        print(json.dumps({"ok": True, "archive": output.relative_to(workspace).as_posix(), "files": len(members) + 1, "missing_notices": len(missing)}, sort_keys=True))
        return 0
    except (OSError, ValueError, PackagingError, zipfile.BadZipFile) as error:
        if created:
            try:
                checked(output, workspace)
                metadata = output.stat()
                if (metadata.st_dev, metadata.st_ino) == owned_identity:
                    output.unlink()
            except (OSError, PackagingError):
                pass  # Never recursively remove or touch an unattributable path.
        # OSError text can contain user-specific absolute paths; do not emit it.
        reason = str(error) if isinstance(error, PackagingError) else "package input/output or metadata validation failed"
        print(json.dumps({"ok": False, "error": reason}, ensure_ascii=True, sort_keys=True))
        return 1


if __name__ == "__main__":
    sys.exit(main())

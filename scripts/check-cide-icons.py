#!/usr/bin/env python3
"""Read CIDE PE icon resources as data; never execute the inspected binaries."""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import struct
import sys


LIMIT = 4 * 1024 * 1024
SIZES = {16, 24, 32, 48, 64, 128, 256}
BINARIES = (
    ("cide.exe", "cide-tui.ico"),
    ("jc-ide.exe", "cide-tui.ico"),
    ("cide-tui.exe", "cide-tui.ico"),
    ("jc-ide-tui.exe", "cide-tui.ico"),
    ("cide-gui.exe", "cide-gui.ico"),
    ("jc-ide-gui.exe", "cide-gui.ico"),
)


class InvalidIcon(Exception):
    """Static, non-sensitive validation error for the JSON report."""


def confined(path: Path, workspace: Path) -> Path:
    if not path.absolute().is_relative_to(workspace):
        raise InvalidIcon("requested path is outside the workspace")
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(workspace) or not resolved.is_file():
        raise InvalidIcon("expected ordinary file inside the workspace")
    return resolved


def source_icon(path: Path) -> tuple[str, list[tuple[tuple[int, ...], bytes]]]:
    # Bound file reads before allocating; read one extra byte to detect growth.
    if path.stat().st_size > LIMIT:
        raise InvalidIcon("source ICO exceeds 4 MiB")
    with path.open("rb") as stream:
        data = stream.read(LIMIT + 1)
    if len(data) > LIMIT or len(data) < 6:
        raise InvalidIcon("source ICO size is invalid")
    reserved, kind, count = struct.unpack_from("<HHH", data)
    if reserved != 0 or kind != 1 or not 1 <= count <= 16:
        raise InvalidIcon("source ICO header is invalid")
    directory_end = 6 + count * 16
    if directory_end > len(data):
        raise InvalidIcon("source ICO directory is truncated")
    frames = []
    intervals = []
    sizes = set()
    for index in range(count):
        entry = struct.unpack_from("<BBBBHHII", data, 6 + index * 16)
        width, height, colors, zero, planes, bits, length, offset = entry
        actual_width, actual_height = width or 256, height or 256
        if zero != 0 or actual_width != actual_height or actual_width not in SIZES:
            raise InvalidIcon("source ICO dimensions or reserved field are invalid")
        if actual_width in sizes:
            raise InvalidIcon("source ICO repeats a size")
        sizes.add(actual_width)
        if length == 0 or length > LIMIT or offset < directory_end or offset > len(data) - length:
            raise InvalidIcon("source ICO frame range is invalid")
        end = offset + length
        if any(offset < other_end and other_start < end for other_start, other_end in intervals):
            raise InvalidIcon("source ICO frames overlap")
        intervals.append((offset, end))
        frame = data[offset:end]
        if len(frame) < 24 or frame[:8] != b"\x89PNG\r\n\x1a\n" or frame[12:16] != b"IHDR":
            raise InvalidIcon("source ICO frame is not a PNG with IHDR")
        if struct.unpack_from(">II", frame, 16) != (actual_width, actual_height):
            raise InvalidIcon("source PNG dimensions disagree with ICO directory")
        frames.append(((width, height, colors, zero, planes, bits, length), frame))
    if sizes != SIZES:
        raise InvalidIcon("source ICO must contain the seven required sizes")
    return hashlib.sha256(data).hexdigest(), frames


class Resources:
    def __init__(self) -> None:
        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.LoadLibraryExW.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]
        self.api.LoadLibraryExW.restype = wintypes.HMODULE
        # Integer names/types are MAKEINTRESOURCE values, not string pointers.
        self.api.FindResourceW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p]
        self.api.FindResourceW.restype = wintypes.HANDLE
        self.api.SizeofResource.argtypes = [wintypes.HMODULE, wintypes.HANDLE]
        self.api.SizeofResource.restype = wintypes.DWORD
        self.api.LoadResource.argtypes = [wintypes.HMODULE, wintypes.HANDLE]
        self.api.LoadResource.restype = wintypes.HANDLE
        self.api.LockResource.argtypes = [wintypes.HANDLE]
        self.api.LockResource.restype = ctypes.c_void_p
        self.api.FreeLibrary.argtypes = [wintypes.HMODULE]
        self.api.FreeLibrary.restype = wintypes.BOOL

    def read(self, module, identifier: int, kind: int, expected_length: int) -> bytes:
        resource = self.api.FindResourceW(module, ctypes.c_void_p(identifier), ctypes.c_void_p(kind))
        if not resource:
            raise InvalidIcon("required icon resource is missing")
        length = self.api.SizeofResource(module, resource)
        if length == 0 or length > LIMIT or length != expected_length:
            raise InvalidIcon("icon resource has an unexpected or excessive size")
        loaded = self.api.LoadResource(module, resource)
        if not loaded:
            raise InvalidIcon("LoadResource failed")
        pointer = self.api.LockResource(loaded)
        if not pointer:
            raise InvalidIcon("LockResource failed")
        # Resource bytes stay valid until FreeLibrary; only this bounded copy is
        # owned by Python. LoadResource handles must not be GlobalFree'd.
        return ctypes.string_at(pointer, length)

    def verify(self, executable: Path, frames: list[tuple[tuple[int, ...], bytes]]) -> None:
        module = self.api.LoadLibraryExW(str(executable), None, 0x00000002)
        if not module:
            raise InvalidIcon("LoadLibraryExW data-file mapping failed")
        try:
            group = self.read(module, 101, 14, 6 + 14 * len(frames))
            reserved, kind, count = struct.unpack_from("<HHH", group)
            if (reserved, kind, count) != (0, 1, len(frames)):
                raise InvalidIcon("group icon header disagrees with source ICO")
            for index, (expected, png) in enumerate(frames):
                entry = struct.unpack_from("<BBBBHHIH", group, 6 + 14 * index)
                if entry[:-1] != expected or entry[-1] != index + 1:
                    raise InvalidIcon("group icon metadata or image ID disagrees with source ICO")
                if self.read(module, entry[-1], 3, len(png)) != png:
                    raise InvalidIcon("embedded icon image bytes disagree with source ICO")
        finally:
            if not self.api.FreeLibrary(module):
                raise InvalidIcon("FreeLibrary failed for data-file mapping")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=Path("target/release"))
    args = parser.parse_args()
    summary = {
        "scope": "PE icon resources as data only; no GUI startup or HWND-title validation",
        "ok": False,
        "results": [],
    }
    if os.name != "nt":
        summary["error"] = "Windows resource APIs are required"
        print(json.dumps(summary, sort_keys=True))
        return 1
    workspace = Path(__file__).resolve().parents[1]
    requested = args.bin_dir if args.bin_dir.is_absolute() else workspace / args.bin_dir
    try:
        # Validate the complete input set before loading any executable as data.
        executables = {name: confined(requested / name, workspace) for name, _ in BINARIES}
        sources = {
            name: source_icon(confined(workspace / "assets" / "branding" / name, workspace))
            for name in {icon for _, icon in BINARIES}
        }
        resources = Resources()
    except (OSError, ValueError, InvalidIcon):
        summary["error"] = "inputs must be six existing workspace binaries and valid bounded source ICOs"
        print(json.dumps(summary, sort_keys=True))
        return 1
    for name, icon in BINARIES:
        digest, frames = sources[icon]
        result = {"name": name, "icon_source": icon, "icon_source_sha256": digest, "ok": False}
        try:
            resources.verify(executables[name], frames)
            result.update(ok=True, frame_count=len(frames), sizes=sorted(SIZES))
        except InvalidIcon as error:
            result["error"] = str(error)
        except (OSError, ValueError, struct.error):
            result["error"] = "Windows resource verification failed"
        summary["results"].append(result)
    summary["ok"] = len(summary["results"]) == 6 and all(row["ok"] for row in summary["results"])
    print(json.dumps(summary, ensure_ascii=True, sort_keys=True))
    return 0 if summary["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())

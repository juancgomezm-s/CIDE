#!/usr/bin/env python3
"""Check CIDE/legacy --help equivalence without starting an interactive frontend.

Build the binaries separately before invoking this opt-in script. This verifies
help only, not HWND titles, rendering or interactive GUI behavior.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import threading


OUTPUT_LIMIT = 16 * 1024  # combined stdout/stderr retained per invocation
TIMEOUT_SECONDS = 15
PAIRS = (
    ("cide", "jc-ide"),
    ("cide-tui", "jc-ide-tui"),
    ("cide-gui", "jc-ide-gui"),
)


def confined(path: Path, workspace: Path) -> Path:
    if not path.absolute().is_relative_to(workspace):
        raise ValueError("requested path is outside the workspace")
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(workspace):
        raise ValueError("executable path is outside the workspace")
    return resolved


def help_output(executable: Path) -> tuple[dict, bytes]:
    """Drain both pipes concurrently, retaining at most OUTPUT_LIMIT bytes.

    A runaway child is killed as soon as its combined output crosses the cap.
    Only the exact process created here is ever signalled.
    """
    result = {"name": executable.name, "ok": False}
    flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
    try:
        process = subprocess.Popen(
            [str(executable), "--help"],
            cwd=executable.parent,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            creationflags=flags,
            shell=False,
        )
    except OSError:
        result["error"] = "could not launch owned executable"
        return result, b""

    stdout = bytearray()
    stderr = bytearray()
    lock = threading.Lock()
    overflow = threading.Event()
    read_error = threading.Event()

    def kill_owned() -> None:
        try:
            process.kill()
        except OSError:
            pass  # It may have exited between detecting the error and killing.

    def drain(pipe, destination: bytearray) -> None:
        try:
            with pipe:
                while True:
                    chunk = pipe.read(1024)
                    if not chunk:
                        break
                    with lock:
                        remaining = OUTPUT_LIMIT - len(stdout) - len(stderr)
                        destination.extend(chunk[:remaining])
                        if len(chunk) > remaining:
                            overflow.set()
                    if overflow.is_set():
                        kill_owned()
                        break
        except (OSError, ValueError):
            read_error.set()
            kill_owned()

    # Daemon is a final exit safeguard if an unexpected descendant inherits a
    # pipe handle. Normal completion must still join both readers below.
    readers = [
        threading.Thread(target=drain, args=(process.stdout, stdout), daemon=True),
        threading.Thread(target=drain, args=(process.stderr, stderr), daemon=True),
    ]
    for reader in readers:
        reader.start()
    timed_out = False
    cleanup_failed = False
    try:
        process.wait(timeout=TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        timed_out = True
        kill_owned()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            cleanup_failed = True
    for reader in readers:
        reader.join(timeout=1)
    if any(reader.is_alive() for reader in readers):
        cleanup_failed = True
        kill_owned()
    with lock:
        output = bytes(stdout)
        errors = bytes(stderr)
    result.update(
        exit_code=process.returncode,
        stdout_bytes=len(output),
        stderr_bytes=len(errors),
        help_sha256=hashlib.sha256(output).hexdigest(),
    )
    if timed_out:
        result["error"] = "help timed out"
    elif cleanup_failed:
        result["error"] = "owned process or output readers did not finish"
    elif overflow.is_set():
        result["error"] = "help exceeded combined 16 KiB output limit"
    elif read_error.is_set():
        result["error"] = "could not read help output"
    elif process.returncode != 0:
        result["error"] = "help returned a nonzero exit code"
    else:
        result["ok"] = True
    return result, output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--bin-dir",
        type=Path,
        default=Path("target/release"),
        help="existing executable directory inside this workspace (default: target/release)",
    )
    args = parser.parse_args()
    workspace = Path(__file__).resolve().parents[1]
    suffix = ".exe" if os.name == "nt" else ""
    summary = {"scope": "help only; no interactive GUI or HWND-title validation", "ok": False, "results": []}
    try:
        requested = args.bin_dir if args.bin_dir.is_absolute() else workspace / args.bin_dir
        directory = confined(requested, workspace)
        if not directory.is_dir():
            raise ValueError("binary directory is not a directory")
        executables = {}
        # Validate all six before launching any. A symlink to outside the root
        # fails the same confinement check as an explicitly external directory.
        for pair in PAIRS:
            for name in pair:
                executable = confined(directory / (name + suffix), workspace)
                if not executable.is_file():
                    raise ValueError("expected executable is not a file")
                executables[name] = executable
    except (OSError, ValueError):
        summary["error"] = "all six executable files must exist strictly inside the workspace"
        print(json.dumps(summary, ensure_ascii=True, sort_keys=True))
        return 1

    for canonical, legacy in PAIRS:
        pair_results = []
        pair_outputs = []
        for name in (canonical, legacy):
            result, output = help_output(executables[name])
            # Report the requested alias even if its file resolves to a symlink.
            result["name"] = name + suffix
            if result["ok"]:
                try:
                    text = output.decode("utf-8", errors="strict")
                except UnicodeDecodeError:
                    result.update(ok=False, error="help is not strict UTF-8")
                else:
                    if "CIDE" not in text or canonical not in text or legacy not in text:
                        result.update(ok=False, error="help omits CIDE, canonical command or legacy alias")
                    elif canonical == "cide" and "no gui yet" in text.lower():
                        result.update(ok=False, error="CLI help still denies the separate GUI")
            pair_results.append(result)
            pair_outputs.append(output)
        equivalent = pair_outputs[0] == pair_outputs[1]
        for result in pair_results:
            result["alias_help_equal"] = equivalent
            if not equivalent:
                result.update(ok=False, error="canonical and legacy help differ")
        summary["results"].extend(pair_results)
    summary["ok"] = len(summary["results"]) == 6 and all(row["ok"] for row in summary["results"])
    print(json.dumps(summary, ensure_ascii=True, sort_keys=True))
    return 0 if summary["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())

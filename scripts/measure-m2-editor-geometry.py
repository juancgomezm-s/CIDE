"""Measure CIDE GUI geometry in one owned hidden native process."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    executable = (root / args.exe).resolve(strict=True)
    if not executable.is_relative_to(root) or executable.name != "cide-gui.exe":
        parser.error("usa cide-gui.exe compilado dentro del proyecto")

    work = root / "work"
    if work.resolve(strict=True) != work or work.stat().st_file_attributes & 0x400:
        parser.error("work debe ser un directorio ordinario del proyecto")
    folder = work / ("m2-geometry-" + uuid.uuid4().hex)
    folder.mkdir()
    guard = folder / "fixture.txt"
    guard_bytes = b"alpha\r\nbeta\r\n"
    guard.write_bytes(guard_bytes)
    source = folder / "viewport.txt"
    line = b"CIDE M2 geometry | native composition | horizontal and vertical ranges.\r\n"
    original = (line * (1 + (256 * 1024 // len(line))))[:256 * 1024]
    source.write_bytes(original)
    native = folder / "native.json"
    with (folder / "stdout.log").open("xb") as stdout, (folder / "stderr.log").open("xb") as stderr:
        process = subprocess.Popen(
            [str(executable), "--encoding", "utf8", "--smoke-fixture", str(guard),
             "--smoke-report", str(native), "--smoke-performance-geometry-only", str(source)],
            cwd=folder, env=dict(os.environ, CIDE_NO_SESSION="1", CIDE_STATE_DIR=str(folder / "state")),
            stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        exit_code = process.wait(timeout=90)
    unchanged = source.read_bytes() == original and guard.read_bytes() == guard_bytes
    report = folder / "report.json"
    evidence = dict(
        executable=str(executable), executable_sha256=hashlib.sha256(executable.read_bytes()).hexdigest(),
        exit_code=exit_code, source_bytes=len(original), sources_unchanged=unchanged,
        native=json.loads(native.read_text(encoding="utf-8")) if native.is_file() else None,
    )
    report.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(report, flush=True)
    if exit_code != 0 or not unchanged or not evidence["native"] or not evidence["native"]["ok"]:
        raise SystemExit("La prueba de geometría falló. Revisa el informe y stderr.log.")


if __name__ == "__main__":
    main()

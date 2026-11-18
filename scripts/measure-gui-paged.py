"""Mide la apertura y los saltos por byte de CIDE en una ventana de prueba oculta."""

import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import stat
import statistics
import subprocess
import sys
import tempfile
import time


FAR_OFFSET = (1 << 32) + 65_536
FIRST_MARKER = b"MARCADOR_INICIO_CIDE_GB\r\n"
LAST_MARKER = b"MARCADOR_FINAL_CIDE_GB\r\n"
SMOKE_FIXTURE = b"alpha\r\nbeta\r\n"
EXPECTED_SIZE = FAR_OFFSET + len(LAST_MARKER)


def create_sparse_source(path):
    import msvcrt

    fsctl_set_sparse = 0x000900C4
    control = c.WinDLL("kernel32", use_last_error=True).DeviceIoControl
    control.argtypes = (
        w.HANDLE, w.DWORD, c.c_void_p, w.DWORD, c.c_void_p, w.DWORD,
        c.POINTER(w.DWORD), c.c_void_p,
    )
    control.restype = w.BOOL
    with path.open("xb") as source:
        size = w.DWORD()
        if not control(msvcrt.get_osfhandle(source.fileno()), fsctl_set_sparse,
                       None, 0, None, 0, c.byref(size), None):
            raise c.WinError(c.get_last_error())
        source.write(FIRST_MARKER)
        source.seek(FAR_OFFSET - 2)
        source.write(b"\r\n" + LAST_MARKER)


def verify_source(path, initial_mtime_ns):
    metadata = path.stat()
    with path.open("rb") as source:
        first = source.read(len(FIRST_MARKER))
        source.seek(FAR_OFFSET - 2)
        last = source.read(len(LAST_MARKER) + 3)
    if (metadata.st_size != EXPECTED_SIZE or metadata.st_mtime_ns != initial_mtime_ns
            or first != FIRST_MARKER or last != b"\r\n" + LAST_MARKER):
        raise RuntimeError("Cambió la longitud, fecha o marcadores del archivo de prueba")


def distribution(values):
    ordered = sorted(values)
    return dict(count=len(ordered), median_ms=statistics.median(ordered),
                p95_ms=ordered[math.ceil(.95 * len(ordered)) - 1],
                max_ms=ordered[-1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != "win32" or c.sizeof(c.c_void_p) != 8:
        parser.error("esta medición requiere Windows de 64 bits")
    root = Path(__file__).resolve().parents[1]
    exe = (root / args.exe).resolve(strict=True)
    if not exe.is_relative_to(root) or exe.name != "cide-gui.exe" or not exe.is_file():
        parser.error("use el cide-gui.exe compilado dentro del proyecto")

    work = root / "work"
    temporary = work / "test-temp"
    for directory in (work, temporary):
        directory.mkdir(exist_ok=True)
        if (not directory.resolve(strict=True).is_relative_to(root)
                or directory.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT):
            raise RuntimeError("El directorio de prueba no es una carpeta propia del proyecto")
    folder = Path(tempfile.mkdtemp(prefix="cide-gb-gui-timing-", dir=temporary))
    fixture = folder / "smoke-fixture.txt"
    source = folder / "multi-gib.txt"
    native_report = folder / "native.json"
    report_path = folder / "report.json"
    report = {
        "executable": str(exe),
        "executable_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
        "windows": platform.version(),
        "visible": False,
        "physical_display_verified": False,
        "native_report": str(native_report),
    }
    process = None
    try:
        fixture.write_bytes(SMOKE_FIXTURE)
        create_sparse_source(source)
        initial_mtime_ns = source.stat().st_mtime_ns
        environment = dict(os.environ, CIDE_NO_SESSION="1",
                           CIDE_STATE_DIR=str(folder / "state"),
                           TEMP=str(folder), TMP=str(folder))
        started = time.perf_counter()
        with (folder / "stdout.log").open("xb") as stdout, (folder / "stderr.log").open("xb") as stderr:
            process = subprocess.Popen(
                [str(exe), "--encoding", "utf8", "--smoke-fixture", str(fixture),
                 "--smoke-report", str(native_report), "--smoke-performance-paged-only", str(source)],
                cwd=folder, env=environment, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            )
            report["pid"] = process.pid
            code = process.wait(timeout=180)
        report["process_wall_ms"] = (time.perf_counter() - started) * 1000
        report["exit_code"] = code
        if code != 0:
            raise RuntimeError(f"CIDE salió con código {code}. Consulta {folder / 'stderr.log'}")
        native = json.loads(native_report.read_text(encoding="utf-8"))
        jump_samples = native["jump_samples_ms"]
        if (native["ok"] is not True or native["visible"] is not False
                or native["file_bytes"] != EXPECTED_SIZE or native["far_offset"] != FAR_OFFSET
                or native["native_compositions"] < 13 or native["paint_errors"] != 0
                or len(jump_samples) != 12
                or not all(math.isfinite(value) and value > 0 for value in jump_samples)
                or not math.isfinite(native["open_ms"]) or native["open_ms"] <= 0):
            raise RuntimeError("El informe nativo no confirmó la apertura y los saltos solicitados")
        verify_source(source, initial_mtime_ns)
        if fixture.read_bytes() != SMOKE_FIXTURE:
            raise RuntimeError("Cambió el archivo de muestra del medidor")
        report.update(file_bytes=EXPECTED_SIZE, far_offset=FAR_OFFSET,
                      open_ms=native["open_ms"], jump_to_native_paint=distribution(jump_samples),
                      native_compositions=native["native_compositions"],
                      max_private_mib=max(phase["private"] for phase in native["memory"]) / 2**20,
                      fixture_length_mtime_and_markers_verified=True, ok=True)
    except BaseException as error:
        report.update(ok=False, error=repr(error),
                      owned_process_still_running=process is not None and process.poll() is None)
        raise
    finally:
        if process is None or process.poll() is not None:
            for temporary_file in (source, fixture):
                temporary_file.unlink(missing_ok=True)
        report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({**report, "report": str(report_path)}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()

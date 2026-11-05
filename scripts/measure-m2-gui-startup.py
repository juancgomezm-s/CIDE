"""Sample process-to-responsive-HWND and private memory on an owned GUI fixture."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time
import uuid


class MemoryCounters(c.Structure):
    _fields_ = [
        ("size", w.DWORD), ("faults", w.DWORD),
        *[(name, c.c_size_t) for name in (
            "peak_working", "working", "peak_paged_pool", "paged_pool",
            "peak_nonpaged_pool", "nonpaged_pool", "pagefile", "peak_pagefile",
            "private",
        )],
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--visible", action="store_true",
                        help="medir hasta ventana visible y bucle de mensajes en espera")
    parser.add_argument("--lines", type=int, default=315,
                        help="cantidad de líneas sintéticas del archivo")
    args = parser.parse_args()
    if not 1 <= args.lines <= 1_000_000:
        parser.error("use entre 1 y 1 000 000 líneas")
    root = Path(__file__).resolve().parents[1]
    exe = (root / args.exe).resolve(strict=True)
    if not exe.is_relative_to(root) or exe.name != "cide-gui.exe":
        parser.error("use el cide-gui.exe compilado dentro del proyecto")
    base = root / "work"
    output = base / ("m2-gui-startup-" + uuid.uuid4().hex)
    output.mkdir()
    if not output.resolve().is_relative_to(root):
        raise RuntimeError("La carpeta de prueba quedó fuera del proyecto")
    source = output / "prueba.txt"
    original = b"Texto CIDE.\r\n" * args.lines
    with source.open("xb") as file:
        file.write(original)
    ui = c.WinDLL("user32", use_last_error=True)
    memory = c.WinDLL("psapi", use_last_error=True)
    ui.FindWindowW.argtypes = [w.LPCWSTR, w.LPCWSTR]
    ui.FindWindowW.restype = w.HWND
    ui.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
    ui.IsWindowVisible.argtypes = [w.HWND]
    ui.WaitForInputIdle.argtypes = [w.HANDLE, w.DWORD]
    ui.WaitForInputIdle.restype = w.DWORD
    ui.SendMessageTimeoutW.argtypes = [
        w.HWND, w.UINT, w.WPARAM, w.LPARAM, w.UINT, w.UINT, c.POINTER(c.c_size_t)
    ]
    ui.SendMessageTimeoutW.restype = w.LPARAM
    memory.GetProcessMemoryInfo.argtypes = [w.HANDLE, c.POINTER(MemoryCounters), w.DWORD]
    memory.GetProcessMemoryInfo.restype = w.BOOL
    samples = []
    for index in range(8):
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 1 if args.visible else 0
        started = time.perf_counter()
        with subprocess.Popen(
            [str(exe), "--encoding", "utf8", str(source)], cwd=root,
            env=dict(os.environ, CIDE_NO_SESSION="1"),
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, startupinfo=startup,
        ) as process:
            try:
                process_handle = w.HANDLE(int(process._handle))
                if ui.WaitForInputIdle(process_handle, 5000) != 0:
                    raise TimeoutError("La GUI de prueba no entró al bucle")
                deadline = time.monotonic() + 5
                hwnd = None
                while not hwnd and time.monotonic() < deadline and process.poll() is None:
                    hwnd = ui.FindWindowW(f"JCIDE.Native.{process.pid}", None)
                    if not hwnd:
                        time.sleep(0.002)
                pid = w.DWORD()
                if not hwnd or not ui.GetWindowThreadProcessId(hwnd, c.byref(pid)) \
                        or pid.value != process.pid or bool(ui.IsWindowVisible(hwnd)) != args.visible:
                    raise RuntimeError("No se encontró el marco propio de CIDE en el estado solicitado")
                result = c.c_size_t()
                if not args.visible and not ui.SendMessageTimeoutW(hwnd, 0, 0, 0, 2, 5000, c.byref(result)):
                    raise c.WinError(c.get_last_error())
                elapsed = (time.perf_counter() - started) * 1000
                counters = MemoryCounters()
                counters.size = c.sizeof(counters)
                if not memory.GetProcessMemoryInfo(process_handle, c.byref(counters), counters.size):
                    raise c.WinError(c.get_last_error())
                if args.visible:
                    process.terminate()
                    process.wait(timeout=5)
                else:
                    if not ui.SendMessageTimeoutW(hwnd, 0x10, 0, 0, 2, 5000, c.byref(result)):
                        raise c.WinError(c.get_last_error())
                    if process.wait(timeout=5) != 0:
                        raise RuntimeError("La GUI de prueba no cerró correctamente")
                samples.append(dict(ms=elapsed, private_bytes=counters.private, warmup=index == 0))
            finally:
                if process.poll() is None:
                    process.kill()
    assert source.read_bytes() == original
    measured = samples[1:]
    times = sorted(sample["ms"] for sample in measured)
    report = dict(
        executable=str(exe), windows=platform.version(), fixture_bytes=len(original),
        output=str(output), samples=samples, median_ms=statistics.median(times),
        p95_ms=times[math.ceil(0.95 * len(times)) - 1],
        max_private_mib=max(sample["private_bytes"] for sample in measured) / 2**20,
        visible_window=args.visible,
        endpoint="visible_and_input_idle" if args.visible else "hidden_and_wm_null",
        physical_display_verified=False,
    )
    with (output / "report.json").open("x", encoding="utf-8") as file:
        json.dump(report, file, indent=2)
    print(json.dumps(report))


if __name__ == "__main__":
    main()

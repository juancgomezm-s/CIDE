"""Measure wheel-to-paint acknowledgement on an owned visible CIDE fixture."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def summary(values):
    ordered = sorted(values)
    return dict(count=len(ordered), median_ms=statistics.median(ordered),
                p95_ms=ordered[math.ceil(len(ordered) * .95) - 1],
                maximum_ms=max(ordered))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--frames", type=int, default=360)
    args = parser.parse_args()
    if not 120 <= args.frames <= 1800 or args.frames % 2:
        parser.error("use una cantidad par de 120 a 1800 cuadros")
    root = Path(__file__).resolve().parents[1]
    exe = (root / args.exe).resolve(strict=True)
    if not exe.is_relative_to(root) or exe.name != "cide-gui.exe":
        parser.error("use el cide-gui.exe compilado dentro del proyecto")
    folder = Path(tempfile.mkdtemp(prefix="m2-visible-scroll-", dir=root / "work"))
    source = folder / "desplazamiento.txt"
    original = "".join(f"Fila {i:06d}. Texto de prueba: café niño.\r\n"
                       for i in range(100000)).encode()
    source.write_bytes(original)
    ui = c.WinDLL("user32", use_last_error=True)
    ui.FindWindowW.argtypes = [w.LPCWSTR, w.LPCWSTR]
    ui.FindWindowW.restype = w.HWND
    ui.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
    ui.IsWindowVisible.argtypes = [w.HWND]
    ui.WaitForInputIdle.argtypes = [w.HANDLE, w.DWORD]
    ui.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    ui.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
    ui.SendMessageTimeoutW.argtypes = [
        w.HWND, w.UINT, w.WPARAM, w.LPARAM, w.UINT, w.UINT, c.POINTER(c.c_size_t)]
    ui.SendMessageTimeoutW.restype = w.LPARAM
    samples = []
    with subprocess.Popen([str(exe), "--encoding", "utf8", str(source)],
                          cwd=folder, env=dict(os.environ, CIDE_NO_SESSION="1"),
                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL) as process:
        try:
            if ui.WaitForInputIdle(w.HANDLE(int(process._handle)), 5000) != 0:
                raise RuntimeError("CIDE no inició su bucle de mensajes")
            hwnd = ui.FindWindowW(f"JCIDE.Native.{process.pid}", None)
            pid = w.DWORD()
            if not hwnd or not ui.IsWindowVisible(hwnd) or not ui.GetWindowThreadProcessId(
                    hwnd, c.byref(pid)) or pid.value != process.pid:
                raise RuntimeError("No se encontró la ventana visible del proceso de prueba")
            client = w.RECT()
            if not ui.GetClientRect(hwnd, c.byref(client)):
                raise c.WinError(c.get_last_error())
            point = w.POINT(client.right // 2, client.bottom // 2)
            if not ui.ClientToScreen(hwnd, c.byref(point)):
                raise c.WinError(c.get_last_error())
            packed = (point.x & 0xffff) | ((point.y & 0xffff) << 16)

            def send(message, wp=0, lp=0):
                result = c.c_size_t()
                if not ui.SendMessageTimeoutW(hwnd, message, wp, lp, 2, 2000, c.byref(result)):
                    raise RuntimeError("CIDE no confirmó el mensaje de prueba")

            send(0x000f)  # Complete initial WM_PAINT before the measured sweep.
            start = time.perf_counter()
            for index in range(args.frames):
                due = start + index / 60
                delay = due - time.perf_counter()
                if delay > 0:
                    time.sleep(delay)
                if process.poll() is not None or not ui.IsWindowVisible(hwnd):
                    raise RuntimeError("La ventana de prueba dejó de estar visible")
                sent = time.perf_counter()
                delta = -480 if index < args.frames // 2 else 480
                send(0x020a, (delta & 0xffff) << 16, packed)  # Owned HWND wheel input.
                send(0x000f)  # Acknowledged preparation, drawing and EndPaint.
                finished = time.perf_counter()
                samples.append(dict(index=index, sent_ms=(sent-start)*1000,
                                    latency_ms=(finished-sent)*1000,
                                    late_ms=max(0, (sent-due)*1000)))
            send(0x0010)
            if process.wait(timeout=5) != 0:
                raise RuntimeError("La ventana de prueba no cerró correctamente")
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
    if source.read_bytes() != original:
        raise RuntimeError("La prueba modificó el archivo sintético")
    report = dict(executable=str(exe), fixture_bytes=len(original), visible=True,
                  synthetic_wheel=True, requested_hz=60, frames=args.frames,
                  elapsed_ms=samples[-1]["sent_ms"] + samples[-1]["latency_ms"],
                  wheel_to_paint=summary([row["latency_ms"] for row in samples]),
                  scheduling_lateness=summary([row["late_ms"] for row in samples]),
                  above_16_67_ms=sum(row["latency_ms"] > 1000/60 for row in samples),
                  physical_display_verified=False, fixture_unchanged=True,
                  samples=samples)
    output = folder / "report.json"
    output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    del report["samples"]
    print(json.dumps(dict(report=report, output=str(output))))


if __name__ == "__main__":
    main()

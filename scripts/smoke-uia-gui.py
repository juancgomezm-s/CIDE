"""Exercise the Windows UI Automation client against a hidden owned CIDE window."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import os
from pathlib import Path
import subprocess
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    exe = (root / args.exe).resolve(strict=True)
    if not exe.is_relative_to(root) or exe.name != "cide-gui.exe":
        parser.error("use el cide-gui.exe compilado dentro del proyecto")
    work = (root / "work").resolve(strict=True)
    if not work.is_relative_to(root):
        raise RuntimeError("El directorio de prueba está fuera del proyecto")
    output = work / ("uia-gui-" + uuid.uuid4().hex)
    output.mkdir()
    client = output / "cliente.exe"
    framework = Path(os.environ["SystemRoot"]) / "Microsoft.NET/Framework64/v4.0.30319"
    subprocess.run([
        str(framework / "csc.exe"), "/nologo", "/codepage:65001", f"/out:{client}",
        *[f"/reference:{framework / 'WPF' / name}" for name in (
            "UIAutomationClient.dll", "UIAutomationTypes.dll", "WindowsBase.dll",
        )],
        str(root / "scripts/smoke-uia-gui.cs"),
    ], check=True, timeout=20)
    source = output / "uia-prueba.txt"
    original = "uno 🙂\r\ndos".encode("utf-8-sig")
    with source.open("xb") as file:
        file.write(original)
    ui = c.WinDLL("user32", use_last_error=True)
    ui.FindWindowW.argtypes = [w.LPCWSTR, w.LPCWSTR]
    ui.FindWindowW.restype = w.HWND
    ui.IsWindowVisible.argtypes = [w.HWND]
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    with subprocess.Popen(
        [str(exe), "--encoding", "utf8", str(source)], cwd=root,
        env=dict(os.environ, CIDE_NO_SESSION="1"), startupinfo=startup,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    ) as process:
        try:
            deadline = time.monotonic() + 5
            hwnd = None
            while not hwnd and process.poll() is None and time.monotonic() < deadline:
                hwnd = ui.FindWindowW(f"JCIDE.Native.{process.pid}", None)
                if not hwnd:
                    time.sleep(0.01)
            if not hwnd or ui.IsWindowVisible(hwnd):
                raise RuntimeError("No se encontró el marco propio oculto de CIDE")
            result = subprocess.run(
                [str(client), str(hwnd), str(process.pid)], capture_output=True, timeout=30,
            )
            (output / "cliente.log").write_bytes(result.stdout + result.stderr)
            print((result.stdout + result.stderr).decode("utf-8", errors="replace"))
            result.check_returncode()
        finally:
            if process.poll() is None:
                process.kill()
    assert source.read_bytes() == original
    print(output)


if __name__ == "__main__":
    main()

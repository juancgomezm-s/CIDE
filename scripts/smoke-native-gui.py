"""Exercise only the built native GUI's own hidden HWND and retained fixtures.

No dependencies, CI, desktop input injection or interaction with other apps.
The report is functional evidence, never a physical-display latency measurement.
"""
import argparse
import datetime as dt
import json
from pathlib import Path
import struct
import subprocess
import sys
import uuid

SOURCE = b"alpha\r\nbeta\r\n"
REQUIRED = {
    "startup_shared_open_bytes", "no_child_edit_control", "utf16_pair_one_undo",
    "native_mouse_caret_without_jump_history",
    "unpaired_utf16_rejected", "selection_internal_clipboard_and_undo",
    "enter_shared_split_one_undo", "join_shared_transaction_one_undo",
    "split_shared_document_undo", "shared_save_exact_fixture_bytes",
    "unsaved_save_routes_to_save_as", "native_save_all_requires_flow",
    "actual_gdi_offscreen_bitmap", "gdi_partial_clip_no_error", "native_paint_handler", "native_paint_no_errors",
}

def ordinary_directory(path):
    info = path.lstat()
    return path.is_dir() and not path.is_symlink() and not (getattr(info, "st_file_attributes", 0) & 0x400)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path)
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("native Win32 smoke requires Windows; no validation inferred")
    workspace = Path(__file__).resolve().parents[1]
    exe = (args.exe or workspace / "target/release/jc-ide-gui.exe").resolve(strict=True)
    if not exe.is_relative_to(workspace) or exe.name != "jc-ide-gui.exe":
        parser.error("use the built jc-ide-gui.exe inside this workspace")
    base = workspace
    for name in ("work", "native-gui-smoke"):
        base = base / name
        base.mkdir(exist_ok=True)
        if not ordinary_directory(base) or not base.resolve().is_relative_to(workspace):
            parser.error("smoke directory must be ordinary and inside the workspace")
    output = base / (dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ-") + uuid.uuid4().hex[:12])
    output.mkdir()
    fixture, report, bitmap = [output / name for name in ("fixture.txt", "native-report.json", "native-frame.bmp")]
    with fixture.open("xb") as stream:
        stream.write(SOURCE)
    with (output / "stdout.txt").open("xb") as stdout, (output / "stderr.txt").open("xb") as stderr:
        result = subprocess.run(
            [str(exe), "--smoke-fixture", str(fixture), "--smoke-report", str(report), "--smoke-bitmap", str(bitmap)],
            cwd=workspace, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            timeout=45, creationflags=subprocess.CREATE_NO_WINDOW,
        )
    if result.returncode:
        raise RuntimeError(f"native smoke exit {result.returncode}; evidence retained at {output}")
    if fixture.read_bytes() != SOURCE:
        raise RuntimeError("native smoke changed the owned fixture bytes")
    payload = json.loads(report.read_text(encoding="utf-8"))
    if payload.get("ok") is not True or not REQUIRED.issubset(payload.get("checks", [])):
        raise RuntimeError(f"native smoke checks missing/failed: {payload}")
    if payload.get("backend") != "Win32/GDI" or payload.get("window") != "own_hidden_hwnd" or payload.get("native_paints", 0) < 1:
        raise RuntimeError("report did not exercise a native window and paint")
    if payload.get("native_paint_errors", -1) != 0:
        raise RuntimeError("native WM_PAINT reported an error")
    if payload.get("physical_display_verified") or payload.get("os_clipboard_verified"):
        raise RuntimeError("smoke cannot certify physical display or OS clipboard")
    data = bitmap.read_bytes()
    if len(data) > 32 * 1024 * 1024 or data[:2] != b"BM" or len(data) < 54:
        raise RuntimeError("invalid or oversized native bitmap")
    offset = struct.unpack_from("<I", data, 10)[0]
    width, height = struct.unpack_from("<ii", data, 18)
    bit_count = struct.unpack_from("<H", data, 28)[0]
    if width <= 0 or not height or bit_count != 32 or not (54 <= offset < len(data)):
        raise RuntimeError("unexpected native bitmap geometry/format")
    pixels = data[offset:]
    colors = {pixels[index:index+3] for index in range(0, len(pixels)-3, 4)}
    if len(colors) < 4:
        raise RuntimeError("native renderer produced an empty/flat bitmap")
    summary = {
        "ok": True, "executable": str(exe), "output": str(output),
        "fixture_bytes": len(SOURCE), "fixture_unchanged": True,
        "native_report": payload, "bitmap_width": width, "bitmap_height": abs(height),
        "bitmap_colors": len(colors), "physical_display_verified": False,
    }
    with (output / "summary.json").open("x", encoding="utf-8") as stream:
        json.dump(summary, stream, ensure_ascii=False, indent=2)
    print(json.dumps(summary, ensure_ascii=False))

if __name__ == "__main__":
    main()

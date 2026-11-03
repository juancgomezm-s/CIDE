"""Exercise only the built native GUI's own hidden HWND and retained fixtures.

No dependencies, CI, desktop input injection or interaction with other apps.
The report is functional evidence, never a physical-display latency measurement.
"""
import argparse
import datetime as dt
import json
import hashlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import uuid

SOURCE = b"alpha\r\nbeta\r\n"
REQUIRED = {
    "startup_shared_open_bytes", "no_child_edit_control", "utf16_pair_one_undo",
    "cide_native_window_title",
    "cide_native_gui_icon_resource",
    "project_dirty_multiline_native_notification", "project_results_gdi_read_only",
    "project_enter_dirty_document_no_reload_one_undo", "project_stale_rejected_refresh_no_hit",
    "project_cancel_no_late_result_revival", "project_changed_disk_fingerprint_rejected",
    "project_root_change_clears_results_source_preserved",
    "project_replace_preview_gdi_read_only", "project_replace_preview_enter_no_commit_cancel",
    "project_replace_open_dirty_closed_exact_eol_one_undo_confirm_once",
    "project_replace_disk_conflict_not_overwritten", "project_replace_one_file_per_task_partial_cancel",
    "project_replace_no_hit_disabled", "project_replace_encoding_stale_preview_denied",
    "project_replace_root_change_drops_plan_source_preserved",
    "project_replace_empty_draft_deletes_one_undo_preserves_eol_disk",
    "project_replace_identical_open_closed_no_revision_history_or_save",
    "git_status_native_delivery_typed_disk_index_no_enter_navigation",
    "git_head_index_distinct_oid_exact_dirty_sources",
    "git_prepare_and_gdi_no_snapshot_admission_or_completion_consumption",
    "git_hunk_enter_navigation_without_edit",
    "git_shared_undo_stale_enter_denied_explicit_refresh",
    "git_cancel_late_notifications_no_result_revival",
    "git_executable_and_root_reconfiguration_clear_accepted_state",
    "explorer_root_native_delivery_no_input_git_excluded",
    "explorer_lazy_disclosure_expand_right_child_native_delivery",
    "explorer_focus_cycle_tab_do_not_edit_source",
    "explorer_actual_paint_mouse_select_double_dirty_reuse",
    "explorer_menu_toggle_preserves_invoking_focus_context",
    "explorer_menu_and_palette_open_to_side_use_selected_node",
    "explorer_gdi_readonly_no_catalog_task_or_hit_admission",
    "explorer_ctrl_enter_side_pin_shared_document_routing",
    "explorer_compact_resize_preserves_split_and_editor_ownership",
    "explorer_refresh_stale_hit_and_cancelled_delivery_rejected",
    "explorer_root_generation_invalidates_node_and_cached_mouse_target",
    "explorer_source_one_undo_disk_preserved_owned_fixture_cleanup",
    "native_mouse_caret_without_jump_history",
    "unpaired_utf16_rejected", "selection_internal_clipboard_and_undo",
    "enter_shared_split_one_undo", "join_shared_transaction_one_undo",
    "split_shared_document_undo", "shared_save_exact_fixture_bytes",
    "unsaved_save_routes_to_save_as", "native_save_all_routes_explicit_flow",
    "save_all_cancel_keeps_pending_new_document", "save_all_explicit_path_preserves_views_and_undo",
    "save_all_conflict_stops_without_overwriting_disk",
    "actual_gdi_offscreen_bitmap", "gdi_partial_clip_no_error", "native_paint_handler", "native_paint_no_errors",
    "quick_open_native_message_delivery", "selector_query_preserves_document",
    "quick_open_dirty_side_shared_undo", "palette_shared_command_one_undo",
    "palette_unavailable_command_rejected", "project_change_cancels_selector",
    "selector_gdi_paint_read_only",
    "encoding_choice_and_cancel_preserve_source_history", "invisibles_native_spans_without_source_edit",
    "multiline_native_paste_find_replace_all_one_undo_save", "multiline_native_no_hit_eol_gestures_and_atomic_quota",
    "multiline_fields_gdi_and_invisibles_read_only",
    "invisibles_toggle_preserves_search_draft",
}

def ordinary_directory(path):
    info = path.lstat()
    return path.is_dir() and not path.is_symlink() and not (getattr(info, "st_file_attributes", 0) & 0x400)

def prepare_git_fixture(output):
    """Write Git setup only in this newly owned smoke directory, never cwd repo."""
    executable = shutil.which("git")
    if not executable:
        raise RuntimeError("existing Git is required for the integrated native fixture; nothing installed")
    executable = Path(executable).resolve(strict=True)
    if not executable.is_absolute() or not executable.is_file() or (output / ".git").exists():
        raise RuntimeError("invalid Git executable or fixture ownership")
    env = {key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATH") if key in os.environ}
    env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0", "GIT_OPTIONAL_LOCKS": "0",
        "GIT_AUTHOR_NAME": "CIDE fixture", "GIT_COMMITTER_NAME": "CIDE fixture",
        "GIT_AUTHOR_EMAIL": "fixture@invalid", "GIT_COMMITTER_EMAIL": "fixture@invalid"})
    def git(*args, data=None):
        result = subprocess.run([str(executable), "-C", str(output), *args], env=env,
            input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=15, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode or len(result.stdout) > 4096 or len(result.stderr) > 16 * 1024:
            raise RuntimeError("owned Git fixture setup failed")
        return result.stdout.decode("ascii").strip()
    git("init", "--quiet", "--template=", "--initial-branch=main")
    head = git("hash-object", "-w", "--stdin", data=b"base\r\nbeta\r\n")
    git("update-index", "--add", "--cacheinfo", f"100644,{head},fixture.txt")
    tree = git("write-tree")
    commit = git("commit-tree", tree, data=b"owned CIDE native fixture\n")
    git("update-ref", "refs/heads/main", commit)
    index = git("hash-object", "-w", "--stdin", data=SOURCE)
    git("update-index", "--cacheinfo", f"100644,{index},fixture.txt")
    assert head != index
    return executable, {"head_blob":head,"index_blob":index,"commit":commit}

def git_metadata(output):
    """Hash all initial Git files, including index/config/objects/refs."""
    git = output / ".git"
    if not ordinary_directory(git):
        raise RuntimeError("fixture .git ownership changed")
    files = {}
    for directory, names, leaves in os.walk(git, followlinks=False):
        base = Path(directory)
        if not ordinary_directory(base) or any(not ordinary_directory(base / name) for name in names):
            raise RuntimeError("unexpected reparse directory in owned Git fixture")
        for name in leaves:
            path = base / name
            info = path.lstat()
            if path.is_symlink() or getattr(info,"st_file_attributes",0) & 0x400 or not path.is_file() or info.st_size > 4 * 1024 * 1024:
                raise RuntimeError("unexpected metadata leaf in owned Git fixture")
            files[path.relative_to(git).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
            if len(files) > 256:
                raise RuntimeError("Git fixture metadata quota")
    return files

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path)
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("native Win32 smoke requires Windows; no validation inferred")
    workspace = Path(__file__).resolve().parents[1]
    exe = (args.exe or workspace / "target/release/cide-gui.exe").resolve(strict=True)
    if not exe.is_relative_to(workspace) or exe.name not in {"cide-gui.exe", "jc-ide-gui.exe"}:
        parser.error("use the built cide-gui.exe or compatible jc-ide-gui.exe inside this workspace")
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
    git_executable, git_identity = prepare_git_fixture(output)
    git_before = git_metadata(output)
    with (output / "stdout.txt").open("xb") as stdout, (output / "stderr.txt").open("xb") as stderr:
        result = subprocess.run(
            [str(exe), "--git-executable", str(git_executable), "--smoke-fixture", str(fixture), "--smoke-report", str(report), "--smoke-bitmap", str(bitmap)],
            cwd=workspace, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            timeout=45, creationflags=subprocess.CREATE_NO_WINDOW,
        )
    if result.returncode:
        raise RuntimeError(f"native smoke exit {result.returncode}; evidence retained at {output}")
    if fixture.read_bytes() != SOURCE:
        raise RuntimeError("native smoke changed the owned fixture bytes")
    if git_metadata(output) != git_before:
        raise RuntimeError("native GUI changed owned Git metadata/index/config/objects/refs")
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
        "git_fixture":git_identity,"git_metadata_unchanged":True,"git_metadata_sha256":git_before,
        "git_executable":str(git_executable),
    }
    with (output / "summary.json").open("x", encoding="utf-8") as stream:
        json.dump(summary, stream, ensure_ascii=False, indent=2)
    print(json.dumps(summary, ensure_ascii=False))

if __name__ == "__main__":
    main()

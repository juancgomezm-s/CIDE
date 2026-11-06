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
    "git_status_native_delivery_comparison_reuses_dirty_document",
    "git_head_index_distinct_oid_exact_dirty_sources",
    "git_prepare_and_gdi_no_snapshot_admission_or_completion_consumption",
    "git_hunk_enter_navigation_without_edit",
    "git_shared_undo_stale_enter_denied_explicit_refresh",
    "git_cancel_late_notifications_no_result_revival",
    "git_root_reconfiguration_clears_embedded_accepted_state",
    "git_embedded_runtime_requires_no_executable",
    "git_history_root_listing_exact_absent_side_and_pinned_blob",
    "git_history_delivery_gdi_preserves_dirty_document_and_editor_views",
    "git_history_escape_cancels_without_live_source_navigation",
    "git_local_pairs_native_keys_exact_head_index_disk_with_dirty_buffer",
    "git_local_pairs_gdi_and_enter_preserve_sources_and_editor_geometry",
    "git_layout_native_menu_four_arrangements_same_accepted_sources",
    "git_parallel_native_projection_and_gdi_preserve_source_geometry",
    "git_rails_and_divider_native_capture_move_release",
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
    "explorer_native_wheel_preserves_focus_selection_source",
    "explorer_native_stale_viewport_click_rejected",
    "explorer_native_rail_capture_drag_release",
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
GIT_REQUIRED = {name for name in REQUIRED if name.startswith("git_")}
GIT_REQUIRED |= {
    "startup_shared_open_bytes", "cide_native_window_title",
    "cide_native_gui_icon_resource", "no_child_edit_control",
    "native_dpi_changes_96_144_192_preserve_document",
    "actual_gdi_offscreen_bitmap", "native_paint_handler", "native_paint_no_errors",
}
NAVIGATION_REQUIRED = {
    "navigation_menu_palette_shortcut_source_scalars_bom_crlf_invisibles",
    "navigation_invalid_cancel_atomic_field_limit_no_live_preview",
    "navigation_group_local_back_forward_preserves_pin_and_shared_document",
    "navigation_revision_encoding_view_pane_stale_targets_rejected",
    "navigation_multiline_history_flags_replace_draft_menu_palette_preserved",
    "navigation_modal_cancel_accept_disarm_deferred_find_without_stopping_scan",
    "navigation_close_stale_target_original_source_and_undo_preserved",
}
LINE_EDIT_REQUIRED = {
    "line_edit_duplicate_selection_line_bom_one_undo",
    "line_edit_delete_move_caret_selection_eof_contract",
    "line_edit_profile_comments_four_encodings_exact_eol",
    "line_edit_selected_block_and_profile_disabled_comments",
    "line_edit_split_pin_inactive_selection_rebase_readonly",
    "line_edit_modal_explorer_and_stale_palette_no_hidden_edit",
    "line_edit_owned_memory_cleanup_original_source_disk_preserved",
}
SELECTION_REOPEN_REQUIRED = {
    "selection_search_unicode_multiline_four_encodings_flags_preserved",
    "selection_search_invalid_bytes_range_budget_empty_rectangle_denied",
    "selection_search_modal_explorer_stale_palette_no_hidden_navigation",
    "selection_search_cancel_and_view_revision_delivery_not_replayed",
    "reopen_closed_current_disk_same_tab_new_document",
    "reopen_closed_dirty_last_view_new_blank_contract",
    "reopen_closed_error_retry_lifo_metadata_preserved",
    "tabs_index_last_preserve_caret_selection_flags_and_document_identity",
}
RECTANGLE_REQUIRED = {
    "rectangle_unicode_native_complete_clusters", "rectangle_unicode_partial_atom_cut_one_undo",
    "rectangle_commands_four_encodings_atomic", "rectangle_internal_clipboard_height_eof",
    "rectangle_delete_backspace_tab_noop", "rectangle_invisibles_readonly_resize",
    "rectangle_modal_explorer_palette_target", "rectangle_split_pin_revision_encoding",
    "rectangle_repertoire_budget_atomic", "rectangle_mouse_capture_versioned",
}
PARITY_REQUIRED = {
    "parity_indent_outdent_four_encodings_undo", "parity_edit_modal_explorer_stale_palette",
    "parity_join_split_focus_and_rectangle", "parity_scroll_link_keyboard_menu_palette_pin",
    "parity_scroll_link_resize_readonly_cleanup",
}
BOTTOM_REQUIRED = {
    "bottom_empty_menu_palette_three_region_focus", "bottom_f3_delivery_navigation_dirty_split_pin",
    "bottom_stale_revision_encoding_path_root_cancel", "bottom_failure_quota_restart_proposals_inert",
    "bottom_visible_rows_resize_hits_readonly", "bottom_original_source_cleanup",
}
SELECTOR_INPUT_REQUIRED = {
    "selector_input_field_edit_clipboard_menu_atomic", "selector_input_limits_unicode_controls_no_partial",
    "selector_input_quick_open_versioned_reuse_side_pin", "selector_input_reentrant_clipboard_target_discard",
    "selector_input_modal_origin_palette_encoding_focus", "selector_input_readonly_resize_cleanup",
}

INTERACTION_REQUIRED = {
    "interaction_linear_drag_encodings_invisibles_undo",
    "interaction_drag_capture_interrupt_stale",
    "interaction_wheel_fraction_link_pin_focus",
    "interaction_native_menu_context_macros_capabilities",
    "interaction_menu_reentry_stale_field_source",
    "interaction_readonly_compact_cleanup",
    "interaction_native_undo_redo_restores_origin_selection_keeps_invoker_focus",
}

MENU_INVOKER_REQUIRED = {"native_menu_palette_find_invoker_roundtrip"}

SYNTAX_REQUIRED = {
    "syntax_shared_abl_multiline_edit_undo",
    "syntax_encoding_invisibles_selection_precedence",
    "syntax_scroll_switch_pending_budget",
    "syntax_readonly_compact_cleanup",
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
    parser.add_argument("--section", choices=("all", "navigation", "line-edit", "selection-reopen", "rectangle", "parity", "bottom", "selector-input", "interaction", "menu-invoker", "syntax", "workflow", "shell", "git"), default="all")
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
            [str(exe), "--smoke-fixture", str(fixture), "--smoke-report", str(report), "--smoke-bitmap", str(bitmap)]
                + (["--smoke-navigation-only"] if args.section == "navigation" else
                   ["--smoke-line-edit-only"] if args.section == "line-edit" else
                   ["--smoke-selection-reopen-only"] if args.section == "selection-reopen" else
                   ["--smoke-rectangle-only"] if args.section == "rectangle" else
                   ["--smoke-parity-only"] if args.section == "parity" else
                   ["--smoke-bottom-only"] if args.section == "bottom" else
                   ["--smoke-selector-input-only"] if args.section == "selector-input" else
                   ["--smoke-interaction-only"] if args.section == "interaction" else
                   ["--smoke-menu-invoker-only"] if args.section == "menu-invoker" else
                   ["--smoke-syntax-only"] if args.section == "syntax" else
                   ["--smoke-workflow-only"] if args.section == "workflow" else
                   ["--smoke-shell-only"] if args.section == "shell" else
                   ["--smoke-git-only"] if args.section == "git" else []),
            cwd=workspace, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            timeout=45, creationflags=subprocess.CREATE_NO_WINDOW,
            env={**os.environ, "PATH": "", "JC_GIT_TEST_EXE": ""},
        )
    if result.returncode:
        raise RuntimeError(f"native smoke exit {result.returncode}; evidence retained at {output}")
    if fixture.read_bytes() != SOURCE:
        raise RuntimeError("native smoke changed the owned fixture bytes")
    if git_metadata(output) != git_before:
        raise RuntimeError("native GUI changed owned Git metadata/index/config/objects/refs")
    payload = json.loads(report.read_text(encoding="utf-8"))
    required = ({"shell_exposure_reuses_owned_frame_without_source_work", "shell_proportional_chrome_pixel_tree_and_rounded_tabs", "shell_grouped_runs_exact_raster_and_retained_surface_probe", "shell_many_open_documents_filtered_identity_selection", "shell_group_tab_pointer_keeps_pin_and_shared_document", "shell_close_shared_view_and_cancel_dirty_final_view", "shell_compact_geometry_keeps_active_document_reachable"} if args.section == "shell" else {"workflow_open_diff_notified_readonly_without_git", "workflow_open_diff_pin_origins_stale_refresh_cancel", "workflow_source_symbols_notification_selection_readonly", "workflow_source_completion_one_undo_and_stale_rejection", "workflow_source_include_confined_side_without_avm", "workflow_snippet_review_before_one_transaction_and_undo", "workflow_snippet_stale_revision_rejects_acceptance", "workflow_markdown_readonly_typed_render_palette_and_focus"} if args.section == "workflow" else NAVIGATION_REQUIRED if args.section == "navigation" else
                LINE_EDIT_REQUIRED if args.section == "line-edit" else
                SELECTION_REOPEN_REQUIRED if args.section == "selection-reopen" else
                RECTANGLE_REQUIRED if args.section == "rectangle" else
                PARITY_REQUIRED if args.section == "parity" else
                BOTTOM_REQUIRED if args.section == "bottom" else
                SYNTAX_REQUIRED if args.section == "syntax" else
                MENU_INVOKER_REQUIRED if args.section == "menu-invoker" else
                INTERACTION_REQUIRED if args.section == "interaction" else
                SELECTOR_INPUT_REQUIRED if args.section == "selector-input" else
                GIT_REQUIRED if args.section == "git" else
                REQUIRED | NAVIGATION_REQUIRED | LINE_EDIT_REQUIRED | SELECTION_REOPEN_REQUIRED | RECTANGLE_REQUIRED | PARITY_REQUIRED | BOTTOM_REQUIRED | SELECTOR_INPUT_REQUIRED | INTERACTION_REQUIRED | MENU_INVOKER_REQUIRED | SYNTAX_REQUIRED)
    if payload.get("ok") is not True or not required.issubset(payload.get("checks", [])):
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
        "section": args.section,
        "fixture_bytes": len(SOURCE), "fixture_unchanged": True,
        "native_report": payload, "bitmap_width": width, "bitmap_height": abs(height),
        "bitmap_colors": len(colors), "physical_display_verified": False,
        "git_fixture":git_identity,"git_metadata_unchanged":True,"git_metadata_sha256":git_before,
        "git_oracle_executable":str(git_executable), "gui_child_path_empty":True,
    }
    with (output / "summary.json").open("x", encoding="utf-8") as stream:
        json.dump(summary, stream, ensure_ascii=False, indent=2)
    print(json.dumps(summary, ensure_ascii=False))

if __name__ == "__main__":
    main()

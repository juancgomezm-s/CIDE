"""Exercise code comparisons in a private Windows terminal using synthetic Git data."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid

sys.dont_write_bytecode = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--host", type=Path, default=Path("target/release/examples/f3_console_host.exe"))
    args = parser.parse_args()
    workspace = Path(__file__).resolve().parents[1]
    executable = (workspace / args.exe).resolve(strict=True)
    host = (workspace / args.host).resolve(strict=True)
    if os.name != "nt" or not executable.is_relative_to(workspace) or not host.is_relative_to(workspace):
        parser.error("use existing Windows binaries inside the workspace")
    git = shutil.which("git.exe")
    if git is None:
        parser.error("fixture preparation requires the installed Git")
    spec = importlib.util.spec_from_file_location("owned_conpty", workspace / "scripts/smoke-explorer-conpty.py")
    terminal = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(terminal)
    parent = (workspace / "work/test-temp").resolve(strict=True)
    root = parent / ("git-code-conpty-" + uuid.uuid4().hex)
    root.mkdir()
    environment = {key: value for key, value in os.environ.items() if not key.upper().startswith("GIT_")}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="NUL", GIT_CONFIG_SYSTEM="NUL")

    def run(*arguments):
        return subprocess.run([git, "-c", "core.hooksPath=NUL", "-c", "core.autocrlf=false", *arguments],
                              cwd=root, env=environment, check=True, capture_output=True, timeout=15,
                              creationflags=subprocess.CREATE_NO_WINDOW).stdout.decode("ascii").strip()

    source = root / "code.txt"
    tail = "".join(f"LINEA_{number:03d}\n" for number in range(100))
    run("init", "--quiet", "--template=", "--initial-branch=main")
    run("config", "user.name", "CIDE Fixture")
    run("config", "user.email", "fixture@example.invalid")
    source.write_bytes(("cabecera\nantes\n" + tail).encode())
    run("add", "--", source.name)
    initial = run("commit-tree", run("write-tree"), "-m", "Primera revision")
    source.write_bytes(("cabecera\ndespues\nagregada\n" + tail).encode())
    run("add", "--", source.name)
    tip = run("commit-tree", run("write-tree"), "-p", initial, "-m", "Segunda revision")
    run("update-ref", "refs/heads/main", tip)
    before = source.read_bytes()

    def metadata():
        return {p.relative_to(root / ".git").as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in sorted((root / ".git").rglob("*")) if p.is_file()}

    original_git = metadata()
    session = None
    report = {"ok": False, "executable": str(executable),
              "executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
              "desktop_used": False, "personal_recovery_used": False,
              "fixture": str(root), "selected_commit": initial, "path_empty": True}
    try:
        session = terminal.Session(root, executable, host, source,
                                   environment=dict(os.environ, PATH="", CIDE_NO_SESSION="1", NO_COLOR=""))
        session.wait(lambda: session.contains("cabecera"))
        # One native input operation: ConPTY's Win32 bridge may expand a
        # bracketed paste into individual key records before crossterm sees it.
        session.send("Q")
        session.wait(lambda: session.contains("Qcabecera"))
        session.menu("a", "Abrir carpeta")
        session.wait(lambda: session.contains("Metadatos del directorio listos"))
        session.send("\x1b[13;28;13;1;8;1_\x1b[13;28;13;0;8;1_")
        session.wait(lambda: session.contains("Carpeta del proyecto seleccionada"))
        session.menu("v", "Git: comparar copia de HEAD")
        session.wait(lambda: session.contains("F4 historial") and session.contains("HEAD"))

        def colors_present():
            colors = {ink[1] for row in session.inks[3:-2] for ink in row[29:]}
            return {(35, 68, 49), (80, 43, 49)} <= colors

        session.wait(colors_present)
        def click_label(label):
            with session.sync:
                points = [(row.index(label), y) for y, cells in enumerate(session.cells)
                          if label in (row := "".join(cells))]
            assert len(points) == 1, (label, points)
            x, y = points[0]
            session.send(f"\x1b[<0;{x + 2};{y + 1}M\x1b[<0;{x + 2};{y + 1}m")

        click_label("[Elegir commit]")
        session.wait(lambda: session.contains("Primera revision") and session.contains("Segunda revision"))
        click_label("Primera revision")
        session.wait(lambda: session.contains("Commit " + initial[:8]) and session.contains("antes")
                     and session.contains("despues") and session.contains("F4 historial"))
        session.wait(colors_present)
        report["wide_frame"] = session.text()
        session.send("\x1b[F")
        session.wait(lambda: session.text().count("LINEA_099") == 2)
        session.resize(80, 24)
        session.send("\x1b[H")
        session.wait(lambda: session.contains("antes") and session.contains("despues"))
        report["compact_frame"] = session.text()
        session.resize(120, 32)
        session.send("v")
        session.wait(lambda: session.contains("antes") and session.contains("despues"))
        session.send("\x1b[200~IGNORAR\x1b[201~")
        click_label("[Cerrar comparación]")
        session.wait(lambda: session.contains("Qcabecera") and not session.contains("F4 historial"))
        session.send("\x1a")
        session.wait(lambda: session.contains("cabecera") and session.contains("despues")
                     and not session.contains("Qcabecera") and not session.contains("IGNORAR"))
        assert source.read_bytes() == before and metadata() == original_git
        session.menu("a", "Salir")
        assert session.api.WaitForSingleObject(session.process.process, 5000) == 0
        report.update(ok=True, sources_unchanged=True, git_unchanged=True,
                      row_backgrounds_verified=True, linked_scroll_verified=True,
                      compact_fallback_verified=True, normal_exit_verified=True,
                      selected_commit_and_dirty_buffer_verified=True, one_undo_verified=True)
        report["visible_controls_mouse_verified"] = True
    except BaseException as error:
        report["error"] = repr(error)
        if session:
            report["last_frame"] = session.text()
            report["observed_backgrounds"] = sorted({ink[1] for row in session.inks for ink in row})
        raise
    finally:
        if session:
            session.close()
        (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"ok": True, "report": str(root / "report.json")}))


if __name__ == "__main__":
    main()

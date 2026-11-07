"""Run the native combined-services probe on an owned, hidden GUI fixture."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import time
import uuid


SOURCE_BYTES = 256 * 2**10
SOURCE_COUNT = 64
NEEDLE = b"CIDE_SERVICE_NEEDLE"
SMOKE_GUARD = b"alpha\r\nbeta\r\n"
EXPECTED_STATUS = (b" M f001.rs", b"M  f002.rs", b" M f003.rs")


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def snapshot(root, *, exclude=()):
    result = {}
    for folder, directories, files in os.walk(root):
        if Path(folder) == root:
            directories[:] = [name for name in directories if name not in exclude]
        for name in sorted(files):
            path = Path(folder) / name
            relative = path.relative_to(root).as_posix()
            if relative in exclude:
                continue
            if path.is_symlink():
                raise RuntimeError(f"La prueba creó un enlace inesperado: {relative}")
            result[relative] = sha256(path)
    return dict(sorted(result.items()))


def source_bytes(index, version=0):
    prefix = f"// Fixture CIDE {index:03d}, versión {version}.\n// ".encode() + NEEDLE + b"\n"
    line = f"// {index:03d} Ámbito de búsqueda del editor y comparación local de Git.\n".encode()
    count, remainder = divmod(SOURCE_BYTES - len(prefix), len(line))
    # ASCII remainder keeps the file valid UTF-8 at the exact requested size.
    tail = (b"//" + b" " * len(line))[:remainder]
    return prefix + line * count + tail


def create_sources(project):
    files = []
    for index in range(SOURCE_COUNT):
        path = project / f"f{index:03d}.rs"
        path.write_bytes(source_bytes(index))
        files.append(path)
    markdown = project / "README.md"
    markdown.write_text(
        "# CIDE: memoria de servicios\n\n![Imagen de medición](image.bmp)\n\n"
        + "\n\n".join(f"{i:03d}. Fila de referencia para el desplazamiento." for i in range(256))
        + "\n",
        encoding="utf-8",
    )
    width = height = 1024
    pixels = bytes((61, 134, 219, 255)) * (width * height)
    bitmap = project / "image.bmp"
    with bitmap.open("xb") as stream:
        stream.write(struct.pack("<2sIHHI", b"BM", 54 + len(pixels), 0, 0, 54))
        stream.write(struct.pack("<IiiHHIIiiII", 40, width, -height, 1, 32, 0,
                                 len(pixels), 0, 0, 0, 0))
        stream.write(pixels)
    return files, markdown, bitmap


def seed_git(project, git, files, markdown, bitmap):
    # Discard ambient Git settings. Only these owned fixture files are staged.
    environment = {key: value for key, value in os.environ.items() if not key.upper().startswith("GIT_")}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_TERMINAL_PROMPT="0", GIT_AUTHOR_DATE="2026-01-01T00:00:00Z",
                       GIT_COMMITTER_DATE="2026-01-01T00:00:00Z")

    def run(*args):
        return subprocess.run(
            [str(git), "-c", "core.autocrlf=false", "-c", "user.name=CIDE fixture",
             "-c", "user.email=cide-fixture@example.invalid", *args],
            cwd=project, env=environment, stdin=subprocess.DEVNULL, capture_output=True,
            check=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW,
        ).stdout

    run("init", "--quiet", "--template=", "--initial-branch=main")
    info = project / ".git" / "info"
    info.mkdir(exist_ok=True)
    (info / "exclude").write_bytes(b"/smoke-fixture.txt\n/native-report.json\n")
    run("add", "--", *(path.name for path in [*files, markdown, bitmap]))
    run("commit", "--quiet", "-m", "Corpus inicial de servicios CIDE")
    for index in (1, 2, 3):
        files[index].write_bytes(source_bytes(index, version=1))
    run("add", "--", files[2].name)
    status = tuple(run("status", "--porcelain=v1", "-z", "--untracked-files=all").rstrip(b"\0").split(b"\0"))
    if status != EXPECTED_STATUS:
        raise RuntimeError(f"El repositorio de prueba no tiene los tres cambios esperados: {status!r}")
    return [entry.decode("ascii") for entry in status]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    executable = (root / args.exe).resolve(strict=True)
    if not executable.is_relative_to(root) or executable.name != "cide-gui.exe":
        parser.error("use cide-gui.exe compilado dentro del proyecto")
    git = shutil.which("git.exe")
    if git is None:
        parser.error("la preparación del repositorio de prueba requiere el Git local")

    work = root / "work"
    if work.resolve(strict=True) != work or work.stat().st_file_attributes & 0x400:
        parser.error("work debe ser un directorio ordinario del proyecto")
    run_root = work / ("m2-services-" + uuid.uuid4().hex)
    project = run_root / "project"
    project.mkdir(parents=True)
    private_state = run_root / "private-state"  # Created exclusively by the Rust probe.
    guard = project / "smoke-fixture.txt"
    guard.write_bytes(SMOKE_GUARD)
    native_report = project / "native-report.json"
    report_path = run_root / "report.json"
    files, markdown, bitmap = create_sources(project)
    status = seed_git(project, git, files, markdown, bitmap)
    opened = [*files[:4], markdown]
    source_excludes = (".git", native_report.name)
    sources_before = snapshot(project, exclude=source_excludes)
    git_before = snapshot(project / ".git")
    report = dict(
        protocol="cide-m2-combined-services-v1", executable=str(executable),
        executable_sha256=sha256(executable), fixture_project=str(project),
        native_report=str(native_report), private_state=str(private_state),
        opened_documents=[path.name for path in opened], source_count=SOURCE_COUNT,
        bytes_per_source=SOURCE_BYTES, search_needle=NEEDLE.decode(),
        expected_search_matches=SOURCE_COUNT, expected_git_status=status,
        fixture_git_tool=str(Path(git).resolve()), visible=False, personal_recovery=False,
        physical_display_verified=False, sources_sha256_before=sources_before,
        git_sha256_before=git_before,
    )
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    environment = dict(os.environ, PATH="", CIDE_NO_SESSION="1", CIDE_STATE_DIR=str(private_state))
    command = [str(executable), "--encoding", "utf8", "--smoke-fixture", str(guard),
               "--smoke-report", str(native_report), "--smoke-performance-services-only",
               *(str(path) for path in opened)]
    started = time.perf_counter()
    process = None
    try:
        with (run_root / "stdout.log").open("xb") as stdout, (run_root / "stderr.log").open("xb") as stderr:
            process = subprocess.Popen(command, cwd=project, env=environment, startupinfo=startup,
                                       stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
            report["pid"] = process.pid
            report["exit_code"] = process.wait(timeout=120)
        report["process_wall_ms"] = (time.perf_counter() - started) * 1000
        if native_report.is_file() and native_report.stat().st_size:
            report["native"] = json.loads(native_report.read_text(encoding="utf-8"))
        if report["exit_code"] != 0 or not report.get("native", {}).get("ok"):
            raise RuntimeError(f"Falló el escenario combinado. Evidencia en {run_root}")
    except Exception as error:
        report["error"] = repr(error)
        if process is not None:
            report["process_retained_if_running"] = process.poll() is None
        raise
    finally:
        sources_after = snapshot(project, exclude=source_excludes)
        git_after = snapshot(project / ".git")
        report.update(sources_sha256_after=sources_after, git_sha256_after=git_after,
                      sources_unchanged=sources_after == sources_before,
                      git_unchanged=git_after == git_before)
        report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    if not report["sources_unchanged"] or not report["git_unchanged"]:
        raise RuntimeError(f"Cambió el repositorio de prueba. Evidencia en {report_path}")
    print(str(report_path))


if __name__ == "__main__":
    main()

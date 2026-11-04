"""Explorador TUI en ConPTY propio: navegación, apertura y evidencia de celdas VT."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import sys
import uuid

os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
sys.dont_write_bytecode = True


def load_module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


base = load_module('browser_owned_conpty', 'smoke-explorer-conpty.py')
visual = load_module('browser_owned_visual', 'smoke-spanish-conpty.py')

# Win32-input VT record: VK_RETURN / scan28 / CR / down/up / left Ctrl / repeat1.
# This preserves the modifier through the owned Windows pseudo console.
CONTROL_ENTER = '\x1b[13;28;13;1;8;1_\x1b[13;28;13;0;8;1_'
ESCAPE_KEY = '\x1b[27;1;27;1;0;1_\x1b[27;1;27;0;0;1_'


def ordinary(path, root):
    metadata = path.lstat()
    assert path.resolve(strict=True).is_relative_to(root)
    assert not path.is_symlink() and not getattr(metadata, 'st_file_attributes', 0) & 0x400


def select_row(session, label):
    session.wait(lambda: session.menu_row(label))
    for _ in range(32):
        with session.sync:
            if session.menu_row(label, selected=True):
                return
            old = [tuple(row) for row in session.styles]
        session.send('\x1b[B')
        session.wait(lambda: old != [tuple(row) for row in session.styles])
    raise AssertionError('No se pudo seleccionar la fila propia: ' + label)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    args = parser.parse_args()
    assert sys.platform == 'win32' and c.sizeof(c.c_void_p) == 8
    root = Path(__file__).resolve().parents[1]
    exe = args.exe if args.exe.is_absolute() else root / args.exe
    exe = exe.resolve(strict=True)
    assert exe.is_relative_to(root) and exe.name == 'cide-tui.exe'
    host = (exe.parent / 'examples/f3_console_host.exe').resolve(strict=True)
    ordinary(exe, root)
    ordinary(host, root)
    parent = root
    for name in ('work', 'browser-conpty'):
        parent = parent / name
        parent.mkdir(exist_ok=True)
        ordinary(parent, root)
    output = parent / (dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-') + uuid.uuid4().hex[:8])
    output.mkdir()
    fixture = output / 'proyecto'
    fixture.mkdir()
    child = fixture / 'subcarpeta'
    child.mkdir()
    source = fixture / 'inicio.md'
    unicode_source = child / 'niño.md'
    original = '# Fuente inicial\r\nSin modificar el documento.\r\n'.encode('utf8')
    nested = '# Archivo español\r\nNiño, café y edición exacta.\r\n'.encode('utf8')
    for path, value in ((source, original), (unicode_source, nested)):
        with path.open('xb') as stream:
            stream.write(value)
    session = None
    checks = []
    stage = 'inicio'
    report = {
        'ok': False, 'physical_display_verified': False,
        'os_clipboard_verified': False, 'avm_started': False,
        'raster_scope': 'decoded VT cells and emitted RGB/attributes, not a physical terminal screenshot',
        'output': str(output),
    }
    try:
        environment = os.environ.copy()
        environment.pop('NO_COLOR', None)
        session = base.Session(root, exe, host, source, environment=environment)
        session.wait(lambda: session.contains('Fuente inicial') and session.contains('Listo'))
        stage = 'abrir_carpeta'
        session.menu('a', 'Abrir carpeta')
        session.wait(lambda: session.contains('Abrir carpeta — explorador') and session.contains('[+] subcarpeta') and session.contains('Metadatos del directorio listos'))
        assert session.contains('Tab cambia ruta/lista') and session.contains('Ctrl+Intro elegir carpeta actual')
        visual.snapshot(session, output / 'browser-vt.png')
        checks.append('tui_folder_browser_visible_rows_spanish_help_and_theme')
        stage = 'navegar_subdirectorio_padre'
        select_row(session, '[+] subcarpeta')
        session.send('\r')
        session.wait(lambda: session.menu_row('..', selected=True) and session.contains('niño.md') and session.contains('Metadatos del directorio listos'))
        session.send('\r')
        session.wait(lambda: session.contains('[+] subcarpeta') and session.contains('Metadatos del directorio listos'))
        checks.append('tui_folder_browser_enter_child_and_parent_row')
        stage = 'elegir_carpeta'
        session.send(CONTROL_ENTER)
        session.wait(lambda: not session.contains('Abrir carpeta — explorador') and session.contains('Carpeta del proyecto seleccionada'))
        checks.append('tui_folder_browser_control_enter_accepts_current_project_folder')
        stage = 'abrir_archivo_unicode'
        session.menu('a', 'Abrir…')
        session.wait(lambda: session.contains('Abrir archivo — explorador') and session.contains('[+] subcarpeta') and session.contains('Metadatos del directorio listos'))
        select_row(session, '[+] subcarpeta')
        session.send('\r')
        session.wait(lambda: session.contains('niño.md') and session.contains('Metadatos del directorio listos'))
        select_row(session, 'niño.md')
        session.send('\r')
        session.wait(lambda: not session.contains('Abrir archivo — explorador') and session.contains('Archivo español') and session.contains('Niño, café'))
        checks.append('tui_file_browser_opens_unicode_native_path_with_exact_source_bytes')
        stage = 'cancelar_redimensionar'
        session.menu('a', 'Abrir carpeta')
        session.wait(lambda: session.contains('Abrir carpeta — explorador') and session.contains('Metadatos del directorio listos'))
        session.resize(40, 12)
        session.wait(lambda: session.contains('Abrir carpeta') and session.contains('Ruta:'))
        visual.snapshot(session, output / 'browser-compact-vt.png')
        session.resize(120, 32)
        session.wait(lambda: session.contains('Abrir carpeta — explorador') and session.contains('[+] subcarpeta'))
        session.send(ESCAPE_KEY)
        session.wait(lambda: not session.contains('Abrir carpeta — explorador') and session.contains('Archivo español'))
        assert not session.contains('niño.md*')
        # A reversible edit of this owned fixture proves focus was handed back
        # to the editor; undo restores the exact buffer and no save is requested.
        session.send('\x1b[HQ')
        session.wait(lambda: session.contains('Q# Archivo español'))
        session.send('\x1a')
        session.wait(lambda: session.contains('# Archivo español') and not session.contains('Q# Archivo español'))
        assert not session.contains('niño.md*')
        checks.append('tui_browser_compact_resize_escape_restores_editor_focus_and_undo')
        stage = 'salir'
        session.menu('a', 'Salir')
        assert session.api.WaitForSingleObject(session.process.process, 5000) == 0
        code = w.DWORD()
        session.check(session.api.GetExitCodeProcess(session.process.process, c.byref(code)))
        assert code.value == 0
        assert source.read_bytes() == original and unicode_source.read_bytes() == nested
        report.update(ok=True, checks=checks, exit_code=code.value, fixtures_unchanged=True, vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error), stage=stage, checks=checks, screen=session.text() if session else '')
        raise
    finally:
        if session:
            session.close()
        with (output / 'report.json').open('x', encoding='utf8', newline='\n') as stream:
            json.dump(report, stream, ensure_ascii=False, indent=2)
        print(json.dumps({key: value for key, value in report.items() if key != 'screen'}, ensure_ascii=False))


if __name__ == '__main__':
    main()

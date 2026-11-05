"""Explorador TUI: rueda/barra/foco en ConPTY privado, sin depósitos personales."""
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

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    'sidebar_owned_conpty', Path(__file__).with_name('smoke-explorer-conpty.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def ordinary(path, root):
    metadata = path.lstat()
    assert path.resolve(strict=True).is_relative_to(root)
    assert not path.is_symlink() and not getattr(metadata, 'st_file_attributes', 0) & 0x400


def mouse(session, code, x, y, release=False):
    session.send(f'\x1b[<{code};{x + 1};{y + 1}' + ('m' if release else 'M'))


def point(session, label):
    with session.sync:
        for y, row in enumerate(session.cells):
            at = ''.join(row).find(label)
            if at >= 0:
                return at, y
    raise AssertionError('Texto propio no visible: ' + label)


def click(session, x, y):
    mouse(session, 0, x, y)
    mouse(session, 0, x, y, release=True)


def tree_rows(session):
    width = min(28, max(18, session.width // 4))
    # Two headers and two footer hints are outside the scrollable list.
    return [''.join(row[:width - 1]) for row in session.cells[4:session.height - 3]]


def tree_has(session, label):
    return any(label in row for row in tree_rows(session))


def selected(session, label):
    width = min(28, max(18, session.width // 4))
    return any(label in ''.join(row[:width - 1]) and any(styles[1:width - 1])
               for row, styles in zip(session.cells[4:session.height - 3],
                                      session.styles[4:session.height - 3]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    args = parser.parse_args()
    assert sys.platform == 'win32' and c.sizeof(c.c_void_p) == 8
    root = Path(__file__).resolve().parents[1]
    exe = (args.exe if args.exe.is_absolute() else root / args.exe).resolve(strict=True)
    host = (exe.parent / 'examples/f3_console_host.exe').resolve(strict=True)
    assert exe.name == 'cide-tui.exe'
    ordinary(exe, root)
    ordinary(host, root)
    parent = root
    for part in ('work', 'sidebar-conpty'):
        parent = parent / part
        parent.mkdir(exist_ok=True)
        ordinary(parent, root)
    output = parent / (dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-') + uuid.uuid4().hex[:8])
    output.mkdir()
    fixture = output / 'proyecto'
    fixture.mkdir()
    originals = {}
    for index in range(50):
        path = fixture / f'archivo{index:02}.txt'
        data = b'SIDEBAR_SOURCE\r\nsegunda linea\r\n' if index == 0 else f'archivo {index:02}\r\n'.encode('ascii')
        with path.open('xb') as stream:
            stream.write(data)
        originals[path] = data
    session = None
    checks = []
    stage = 'inicio'
    report = {'ok': False, 'output': str(output), 'physical_display_verified': False,
              'physical_mouse_verified': False, 'personal_depot_used': False,
              'raster_scope': 'VT cells from an owned hidden ConPTY', 'screens': {}}
    try:
        environment = os.environ.copy()
        environment.pop('NO_COLOR', None)
        environment['CIDE_NO_SESSION'] = '1'
        environment['CIDE_STATE_DIR'] = str(output / 'estado-aislado')
        session = base.Session(root, exe, host, fixture / 'archivo00.txt', environment=environment)
        session.wait(lambda: session.contains('SIDEBAR_SOURCE'))
        stage = 'abrir_proyecto'
        session.menu('a', 'Abrir carpeta')
        session.wait(lambda: session.contains('Abrir carpeta — explorador')
                     and session.contains('Metadatos del directorio listos'))
        click(session, *point(session, '[Elegir carpeta actual]'))
        session.wait(lambda: not session.contains('Abrir carpeta — explorador')
                     and tree_has(session, 'archivo00.txt') and tree_has(session, 'proyecto'))
        checks.append('sidebar_owned_50_file_catalog_delivered')
        stage = 'rueda_sin_cambiar_foco'
        # Restore editor focus and a known source caret before touching the sidebar.
        click(session, *point(session, 'SIDEBAR_SOURCE'))
        session.send('Q')
        session.wait(lambda: session.contains('QSIDEBAR_SOURCE'))
        session.send('\x1a')
        session.wait(lambda: session.contains('SIDEBAR_SOURCE') and not session.contains('QSIDEBAR_SOURCE'))
        mouse(session, 65, 4, 6)
        mouse(session, 65, 4, 6)
        session.wait(lambda: 'archivo05.txt' in tree_rows(session)[0])
        session.send('W')
        session.wait(lambda: session.contains('WSIDEBAR_SOURCE'))
        session.send('\x1a')
        session.wait(lambda: session.contains('SIDEBAR_SOURCE') and not session.contains('WSIDEBAR_SOURCE'))
        assert 'archivo05.txt' in tree_rows(session)[0]
        checks.append('sidebar_wheel_scrolls_six_rows_preserving_editor_focus_and_caret')
        report['screens']['wheel'] = session.text()
        stage = 'teclado_revela_seleccion'
        session.send('\x1b[17~\x1b[B')  # F6 then Down: root -> archivo00, not the visible sixth row.
        session.wait(lambda: selected(session, 'archivo00.txt'))
        checks.append('sidebar_wheel_preserves_selected_node_and_keyboard_reveals_it')
        stage = 'barra_track'
        width = min(28, max(18, session.width // 4))
        last = session.height - 4
        click(session, width - 1, last)
        session.wait(lambda: tree_has(session, 'archivo49.txt') and not tree_has(session, 'archivo00.txt'))
        session.send('\x1b[A')
        session.wait(lambda: selected(session, 'proyecto'))
        checks.append('sidebar_track_click_preserves_selection_and_sidebar_focus')
        stage = 'barra_arrastre'
        # Root is selected at the start: dragging must leave it selected offscreen.
        mouse(session, 0, width - 1, 4)
        mouse(session, 32, width - 1, last)
        mouse(session, 0, width - 1, last, release=True)
        session.wait(lambda: tree_has(session, 'archivo49.txt'))
        report['screens']['drag'] = session.text()
        session.send('\x1b[B')
        session.wait(lambda: selected(session, 'archivo00.txt'))
        checks.append('sidebar_thumb_drag_reaches_last_file_without_changing_selection_or_focus')
        stage = 'compacto'
        session.resize(60, 12)
        session.wait(lambda: tree_has(session, 'archivo00.') and session.contains('SIDEBAR_SOURCE'))
        mouse(session, 65, 4, 6)
        session.wait(lambda: not tree_has(session, 'archivo00.'))
        session.send('\x1b[H')
        session.wait(lambda: selected(session, 'proyecto'))
        checks.append('sidebar_compact_resize_wheel_and_keyboard_reveal')
        report['screens']['compact'] = session.text()
        stage = 'salida_normal'
        session.resize(120, 32)
        session.wait(lambda: selected(session, 'proyecto') and session.contains('SIDEBAR_SOURCE'))
        session.send('\x1b')
        session.wait(lambda: session.contains('Explorador: foco en el editor'))
        session.menu('a', 'Salir')
        if session.api.WaitForSingleObject(session.process.process, 5000) != 0:
            raise TimeoutError('owned terminal exit deadline')
        code = w.DWORD()
        session.check(session.api.GetExitCodeProcess(session.process.process, c.byref(code)))
        assert code.value == 0
        assert all(path.read_bytes() == data for path, data in originals.items())
        checks.append('sidebar_normal_exit_and_all_50_source_files_byte_identical')
        report.update(ok=True, exit_code=code.value, fixtures_unchanged=True, vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error), screen=session.text() if session else '')
        raise
    finally:
        report.update(stage=stage, checks=checks)
        if session:
            session.close()
        with (output / 'report.json').open('x', encoding='utf-8', newline='\n') as stream:
            json.dump(report, stream, ensure_ascii=True, indent=2)
        print(json.dumps({key: value for key, value in report.items() if key not in ('screens', 'screen')}))


if __name__ == '__main__':
    main()

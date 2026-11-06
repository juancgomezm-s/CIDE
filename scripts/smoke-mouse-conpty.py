"""Ratón TUI en ConPTY propio: selección, rueda, menús y explorador sin archivos personales."""
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


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


base = load('mouse_owned_conpty', 'smoke-explorer-conpty.py')
visual = load('mouse_owned_visual', 'smoke-spanish-conpty.py')


class CursorSession(base.Session):
    def __init__(self, *args, **kwargs):
        self.cursor_visible = False
        self.vt_tail = ''
        super().__init__(*args, **kwargs)

    def feed(self, ch):
        self.vt_tail = (self.vt_tail + ch)[-2048:]
        if self.escape == 'csi' and self.sequence == '?25' and ch in ('h', 'l'):
            self.cursor_visible = ch == 'h'
        super().feed(ch)


def point(session, text, sidebar=False):
    with session.sync:
        for y, row in enumerate(session.cells):
            line = ''.join(row)
            if sidebar:
                line = line.split('│', 1)[0]
            at = line.find(text)
            if at >= 0:
                return at, y
    raise AssertionError('Texto propio no visible: ' + text)


def send_mouse(session, code, x, y, release=False):
    # Standard SGR1006 mouse reports; coordinates are terminal cells, one based.
    session.send('\x1b[<' + str(code) + ';' + str(x + 1) + ';' + str(y + 1) + ('m' if release else 'M'))


def click(session, x, y, shift=False):
    code = 4 if shift else 0
    send_mouse(session, code, x, y)
    send_mouse(session, code, x, y, release=True)


def open_file_menu(session):
    click(session, *point(session, 'Archivo'))
    session.wait(lambda: session.menu_row('Abrir carpeta'))


def ordinary(path, root):
    metadata = path.lstat()
    assert path.resolve(strict=True).is_relative_to(root)
    assert not path.is_symlink() and not getattr(metadata, 'st_file_attributes', 0) & 0x400


def eof_cursor_row(session, source_x, value):
    row = ''.join(session.cells[session.y])
    # Forty-eight source lines plus a final CRLF produce blank line 49.
    # Undo restores source selection, not the previous viewport's screen row.
    gutter = row[:source_x].rsplit('│', 1)[-1].strip()
    return (session.contains('MOUSE_LINE_47') and gutter == '49'
            and row[source_x:session.width - 1].strip() == value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    args = parser.parse_args()
    assert sys.platform == 'win32' and c.sizeof(c.c_void_p) == 8
    root = Path(__file__).resolve().parents[1]
    exe = (args.exe if args.exe.is_absolute() else root / args.exe).resolve(strict=True)
    assert exe.name == 'cide-tui.exe'
    host = (exe.parent / 'examples/f3_console_host.exe').resolve(strict=True)
    ordinary(exe, root)
    ordinary(host, root)
    parent = root
    for part in ('work', 'mouse-conpty'):
        parent = parent / part
        parent.mkdir(exist_ok=True)
        ordinary(parent, root)
    output = parent / (dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-') + uuid.uuid4().hex[:8])
    output.mkdir()
    fixture = output / 'proyecto'
    fixture.mkdir()
    child = fixture / 'carpeta'
    child.mkdir()
    source = fixture / 'mouse.txt'
    nested = child / 'nested.txt'
    original = ''.join(f'MOUSE_LINE_{index:02d}\tuno\r\n' for index in range(48)).encode('utf8')
    nested_bytes = b'MOUSE_NESTED\r\nexact bytes\r\n'
    for path, data in ((source, original), (nested, nested_bytes)):
        with path.open('xb') as stream:
            stream.write(data)
    session = None
    checks = []
    stage = 'inicio'
    source_x = source_y = end_y = None
    report = {
        'ok': False, 'physical_display_verified': False, 'physical_mouse_verified': False,
        'os_clipboard_verified': False, 'avm_started': False,
        'raster_scope': 'decoded VT cells and emitted RGB/attributes, not a physical terminal screenshot',
        'output': str(output),
    }
    try:
        environment = os.environ.copy()
        environment.pop('NO_COLOR', None)
        session = CursorSession(root, exe, host, source, environment=environment)
        session.wait(lambda: session.contains('MOUSE_LINE_00') and session.contains('Listo'))
        stage = 'limites_documento_control'
        source_x, source_y = point(session, 'MOUSE_LINE_00')
        session.send('\x1b[1;5F')  # CSI Ctrl+End, not End of the current line.
        session.wait(lambda: session.x == source_x and session.y > source_y
                     and eof_cursor_row(session, source_x, ''))
        end_y = session.y
        session.send('E')
        session.wait(lambda: session.x == source_x + 1
                     and eof_cursor_row(session, source_x, 'E'))
        session.send('\x1a')
        session.wait(lambda: session.x == source_x and eof_cursor_row(session, source_x, ''))
        session.send('\x1b[1;5H')  # CSI Ctrl+Home returns from the trailing CRLF's empty line.
        session.wait(lambda: session.contains('MOUSE_LINE_00')
                     and session.x == source_x and session.y == source_y)
        session.send('B')
        session.wait(lambda: session.contains('BMOUSE_LINE_00')
                     and session.x == source_x + 1 and session.y == source_y)
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('BMOUSE_LINE_00')
                     and session.x == source_x and session.y == source_y)
        checks.append('tui_control_document_boundaries_use_real_csi_and_preserve_final_crlf_after_undo')
        stage = 'clic_fuente'
        x, y = point(session, 'MOUSE_LINE_00')
        click(session, x + 6, y)
        session.send('Q')
        session.wait(lambda: session.contains('MOUSE_QLINE_00'))
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('MOUSE_QLINE_00'))
        checks.append('tui_mouse_click_places_source_caret_and_one_undo_restores_owned_buffer')
        stage = 'arrastre_lineal'
        x, y = point(session, 'MOUSE_LINE_00')
        send_mouse(session, 0, x, y)
        send_mouse(session, 32, x + 5, y)
        send_mouse(session, 0, x + 5, y, release=True)
        session.send('R')
        session.wait(lambda: session.contains('R_LINE_00') and not session.contains('MOUSE_LINE_00'))
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('R_LINE_00'))
        checks.append('tui_mouse_drag_selects_source_bytes_without_a_parallel_edit_buffer')
        stage = 'mayus_clic'
        x, y = point(session, 'MOUSE_LINE_00')
        before = session.bytes
        click(session, x + 1, y)
        # Wait for the prepared frame after the first click; the next click
        # intentionally targets only a published version of the geometry.
        session.wait(lambda: session.bytes > before and session.x == x + 1 and session.y == y)
        click(session, x + 6, y, shift=True)
        session.wait(lambda: all(session.styles[y][x + 1:x + 6]))
        visual.snapshot(session, output / 'mouse-selection-vt.png')
        session.send('S')
        session.wait(lambda: session.contains('MSLINE_00') and not session.contains('MOUSE_LINE_00'))
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('MSLINE_00')
                     and all(session.styles[y][x + 1:x + 6])
                     and session.x == x + 6 and session.y == y)
        checks.append('tui_mouse_shift_click_extends_the_existing_source_anchor')
        stage = 'seleccion_deshacer_rehacer'
        # No new click or focus change: the restored endpoint distinguishes the
        # original forward selection from the same highlighted bytes backwards.
        session.send('\x19')
        session.wait(lambda: session.contains('MSLINE_00') and not session.contains('MOUSE_LINE_00')
                     and not any(session.styles[y][x + 1:x + 6])
                     and session.x == x + 2 and session.y == y)
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('MSLINE_00')
                     and all(session.styles[y][x + 1:x + 6])
                     and session.x == x + 6 and session.y == y)
        visual.snapshot(session, output / 'mouse-selection-undo-vt.png')
        checks.append('tui_mouse_undo_redo_restores_selection_orientation_and_caret_without_refocusing')
        stage = 'rueda'
        x, y = point(session, 'MOUSE_LINE_00')
        before = session.bytes
        click(session, x, y)
        session.wait(lambda: session.bytes > before and session.x == x and session.y == y)
        send_mouse(session, 65, x + 8, y + 2)
        send_mouse(session, 65, x + 8, y + 2)
        session.wait(lambda: 'MOUSE_LINE_06' in ''.join(session.cells[y]))
        send_mouse(session, 64, x + 8, y + 2)
        session.wait(lambda: 'MOUSE_LINE_03' in ''.join(session.cells[y]))
        visual.snapshot(session, output / 'mouse-wheel-vt.png')
        session.send('W')
        session.wait(lambda: session.contains('WMOUSE_LINE_00'))
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('WMOUSE_LINE_00'))
        checks.append('tui_mouse_wheel_moves_viewport_and_preserves_the_source_caret')
        stage = 'menu_y_explorador'
        open_file_menu(session)
        click(session, *point(session, 'Abrir carpeta'))
        session.wait(lambda: session.contains('Abrir carpeta — explorador') and session.contains('[+] carpeta') and session.contains('Metadatos del directorio listos'))
        click(session, *point(session, '[+] carpeta'))
        session.wait(lambda: session.menu_row('[+] carpeta', selected=True))
        click(session, *point(session, '[Entrar]'))
        session.wait(lambda: session.contains('nested.txt') and session.contains('Metadatos del directorio listos'))
        visual.snapshot(session, output / 'mouse-browser-vt.png')
        click(session, *point(session, '[Elegir carpeta actual]'))
        session.wait(lambda: not session.contains('Abrir carpeta — explorador') and session.tree_contains('nested.txt'))
        checks.append('tui_mouse_menu_and_folder_browser_rows_buttons_accept_owned_project')
        stage = 'panel_lateral_pestanas'
        click(session, *point(session, 'nested.txt', sidebar=True))
        session.wait(lambda: session.contains('MOUSE_NESTED'))
        click(session, 2, 1)
        session.wait(lambda: session.contains('MOUSE_LINE_00'))
        open_file_menu(session)
        click(session, *point(session, 'Abrir…'))
        session.wait(lambda: session.contains('Abrir archivo — explorador') and session.contains('Metadatos del directorio listos'))
        click(session, *point(session, '[Cancelar]'))
        session.wait(lambda: not session.contains('Abrir archivo — explorador') and session.contains('MOUSE_LINE_00'))
        checks.append('tui_mouse_sidebar_tabs_and_browser_cancel_restore_the_existing_document')
        stage = 'redimensionar'
        session.resize(80, 24)
        session.wait(lambda: session.contains('MOUSE_LINE_00'))
        x, y = point(session, 'MOUSE_LINE_00')
        click(session, x, y)
        session.send('T')
        session.wait(lambda: session.contains('TMOUSE_LINE_00'))
        session.send('\x1a')
        session.wait(lambda: session.contains('MOUSE_LINE_00') and not session.contains('TMOUSE_LINE_00'))
        session.resize(120, 32)
        session.wait(lambda: session.contains('MOUSE_LINE_00'))
        visual.snapshot(session, output / 'mouse-vt.png')
        checks.append('tui_mouse_after_resize_uses_the_new_prepared_cell_geometry')
        stage = 'salir'
        session.menu('a', 'Salir')
        assert session.api.WaitForSingleObject(session.process.process, 5000) == 0
        code = w.DWORD()
        session.check(session.api.GetExitCodeProcess(session.process.process, c.byref(code)))
        assert code.value == 0 and source.read_bytes() == original and nested.read_bytes() == nested_bytes
        report.update(ok=True, checks=checks, exit_code=code.value, fixtures_unchanged=True, vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error), stage=stage, checks=checks, screen=session.text() if session else '')
        if session:
            report['cursor_diagnostics'] = {'x': session.x, 'y': session.y, 'end_y': end_y,
                                            'source_x': source_x, 'source_y': source_y,
                                            'visible': session.cursor_visible,
                                            'vt_tail': session.vt_tail,
                                            'cursor_row': ''.join(session.cells[session.y])}
        raise
    finally:
        if session:
            session.close()
        with (output / 'report.json').open('x', encoding='utf8', newline='\n') as stream:
            json.dump(report, stream, ensure_ascii=False, indent=2)
        print(json.dumps({key: value for key, value in report.items() if key != 'screen'}, ensure_ascii=True))


if __name__ == '__main__':
    main()

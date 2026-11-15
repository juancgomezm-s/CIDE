"""Cierre y reapertura reales de la TUI con recuperación Rust en un depósito propio."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import sys
import time
import uuid

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('recovery_conpty', Path(__file__).with_name('smoke-explorer-conpty.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)

def read_head(path):
    try:
        data=path.read_bytes()
        return data if len(data)==72 and data[:8]==b'CIDEHD01' else None
    except (FileNotFoundError,PermissionError):
        return None

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--host',type=Path)
    parser.add_argument('--periodic',action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    exe = (root / args.exe).resolve(strict=True)
    assert exe.is_relative_to(root) and exe.name == 'cide-tui.exe'
    host = (root/args.host if args.host else exe.parent/'examples/f3_console_host.exe').resolve(strict=True)
    assert host.is_relative_to(root) and host.name == 'f3_console_host.exe'
    output = root / 'work/recovery-conpty' / (dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-') + uuid.uuid4().hex[:8])
    output.mkdir(parents=True)
    source = output / 'documento.md'
    original = '# Documento original\r\nContenido sin guardar.\r\n'.encode('utf8')
    with source.open('xb') as file:
        file.write(original)
    environment = dict(os.environ, CIDE_NO_SESSION='0', CIDE_STATE_DIR=str(output / 'estado'))
    checks = []
    report = dict(ok=False, output=str(output), original_unchanged=False, physical_display_verified=False,periodic=args.periodic)
    session = None
    try:
        for phase in range(3):
            previous={str(path):path.read_bytes() for path in (output/'estado/tui').glob('slot-*/HEAD')}
            session = base.Session(root, exe, host, source, environment=environment)
            session.wait(lambda: session.contains('Documento original'))
            if phase == 0:
                session.send('\x1b[200~RECUPERADO_\x1b[201~')
                session.wait(lambda: session.contains('RECUPERADO_# Documento original'))
                checks.append('dirty_edit_visible_before_exit')
            elif phase == 1:
                session.wait(lambda: session.contains('RECUPERADO_# Documento original'))
                session.menu('e','Deshacer')
                session.wait(lambda: session.contains('# Documento original') and not session.contains('RECUPERADO_#'))
                checks.append('new_process_restores_dirty_bytes_and_original_undo')
                session.menu('e','Rehacer')
                session.wait(lambda: session.contains('RECUPERADO_# Documento original'))
                checks.append('restored_redo_retains_the_same_change')
            else:
                session.wait(lambda: session.contains('RECUPERADO_# Documento original'))
                checks.append('second_close_and_reopen_keeps_recovered_modification')
            if args.periodic:
                deadline=time.monotonic()+20
                while True:
                    heads=list((output/'estado/tui').glob('slot-*/HEAD'))
                    if heads and any((value:=read_head(path)) is not None and value!=previous.get(str(path)) for path in heads) and len(list(heads[0].parent.glob('generation-*')))<=2:break
                    if time.monotonic()>deadline:raise TimeoutError('No se confirmó el checkpoint periódico y su limpieza TUI')
                    time.sleep(.03)
                if phase==2:
                    stable=read_head(heads[0]);time.sleep(2.3);assert stable is not None and read_head(heads[0])==stable,'La sesión en reposo no debe reescribirse'
            else:
                session.menu('a', 'Salir')
                if session.api.WaitForSingleObject(session.process.process,10000) != 0:
                    raise TimeoutError('No se confirmó el cierre con recuperación')
                code=w.DWORD()
                session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)))
                assert code.value == 0
            assert source.read_bytes() == original
            session.close()
            session = None
        if args.periodic:checks+=['periodic_commit_before_forced_job_termination_without_exit_command','obsolete_generations_pruned_to_two_protected_versions','idle_session_does_not_rewrite_checkpoint']
        report.update(ok=True, checks=checks, original_unchanged=True, processes=3)
    except BaseException as error:
        report.update(error=str(error), checks=checks, screen=session.text() if session else '')
        raise
    finally:
        report['original_unchanged'] = source.read_bytes() == original
        if session:
            session.close()
        (output / 'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
        print(json.dumps(report,ensure_ascii=False))

if __name__ == '__main__':
    main()

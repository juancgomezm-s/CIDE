"""Measure complete native GUI processes using their owned hidden smoke HWND.

The renderer and probe target DIB run IN the editor process. Reports retain raw
process memory including that explicitly counted extra probe DIB. No desktop,
clipboard, personal recovery, external Git, network or force-kill is used.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time
import uuid


def distribution(values):
    ordered = sorted(values)
    return dict(count=len(values), median_ms=statistics.median(ordered),
                p95_ms=ordered[math.ceil(.95*len(ordered))-1], max_ms=max(ordered))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True, type=Path)
    parser.add_argument('--cases', nargs='+', choices=('small', 'dense', 'unicode', 'long', 'multi'),
                        default=['small', 'dense', 'unicode', 'long', 'multi'])
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    exe = (root/args.exe).resolve(strict=True)
    if not exe.is_relative_to(root) or exe.name != 'cide-gui.exe':
        parser.error('use cide-gui.exe compilado dentro del proyecto')
    folder = root/'work'/('m2-editor-'+uuid.uuid4().hex)
    folder.mkdir()
    report = dict(executable=str(exe), executable_sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),
                  windows=platform.version(), visible=False, recovery=False,
                  physical_display_verified=False, endpoint='same_process_WM_PRINTCLIENT_retained_native_DIB',
                  limitations=['No compositor, physical display or key-to-photon timing',
                               'Raw process memory INCLUDES a separately counted extra in-process probe target DIB',
                               'Fresh processes with uncontrolled OS file cache; not cold-start evidence',
                               'CPU idle is sampled while the owned GUI message pump dispatches pending messages',
                               'Optional Git/search/Markdown image services are not activated by this corpus',
                               'Multi visits every retained document; long does not navigate to end of its 16MiB line'],
                  cases=[])
    report_path = folder/'report.json'

    def save():
        report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8')

    for case in args.cases:
        case_folder = folder/case
        case_folder.mkdir()
        fixture = case_folder/'smoke-fixture.txt'
        fixture.write_bytes(b'alpha\r\nbeta\r\n')
        unit = ('Fila café niño Ελληνικά 😀.\r\n'.encode('utf-8') if case == 'unicode'
                else b'abcdefghij\r\n')
        source_bytes = (b'Texto CIDE.\r\n'*315 if case == 'small' else
                        b'x'*(16*2**20) if case == 'long' else
                        unit*((4*2**20)//len(unit)))
        sources = []
        for index in range(8 if case == 'multi' else 1):
            source = case_folder/f'prueba-{index}.txt'
            with source.open('xb') as stream:
                stream.write(source_bytes)
            sources.append(source)
        native_report = case_folder/'native-report.json'
        row = dict(case=case, documents=len(sources), bytes_per_document=len(source_bytes),
                   native_report=str(native_report))
        report['cases'].append(row)
        started = time.perf_counter()
        with (case_folder/'stdout.log').open('xb') as stdout, (case_folder/'stderr.log').open('xb') as stderr:
            process = subprocess.Popen(
                [str(exe), '--encoding', 'utf8', '--smoke-fixture', str(fixture),
                 '--smoke-report', str(native_report), '--smoke-performance-only', *map(str, sources)],
                cwd=case_folder, env=dict(os.environ, CIDE_NO_SESSION='1',
                                         CIDE_STATE_DIR=str(case_folder/'state')),
                stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
            row['pid'] = process.pid
            try:
                code = process.wait(timeout=120)
                row['exit_code'] = code
                if code != 0:
                    raise RuntimeError(f'CIDE devolvió {code}; revisar {case_folder / "stderr.log"}')
                native = json.loads(native_report.read_text(encoding='utf-8'))
                if not native['ok'] or native['visible'] or not native['native_compositions']:
                    raise RuntimeError('No hubo composición nativa oculta verificada')
                if native['documents'] != len(sources):
                    raise RuntimeError('La cantidad de documentos abiertos no coincide')
                if fixture.read_bytes() != b'alpha\r\nbeta\r\n' or any(
                        source.read_bytes() != source_bytes for source in sources):
                    raise RuntimeError('El proceso modificó fuentes privadas')
                row.update(native)
                row['process_wall_ms'] = (time.perf_counter()-started)*1000
                row['sources_unchanged'] = True
                row['max_private_mib'] = max(value['private'] for value in native['memory'])/2**20
                row['peak_commit_mib'] = max(value['peak_pagefile'] for value in native['memory'])/2**20
                row['peak_working_mib'] = max(value['peak_working'] for value in native['memory'])/2**20
                row['scroll_to_native_paint'] = distribution(native['scroll_samples_ms'])
                row['edit_undo_native_paint'] = distribution(native['edit_undo_samples_ms'])
                save()
                print(json.dumps({key: row[key] for key in ('case', 'max_private_mib', 'peak_commit_mib',
                      'scroll_to_native_paint', 'edit_undo_native_paint', 'idle')}, ensure_ascii=False), flush=True)
            except Exception as error:
                row['error'] = repr(error)
                row['process_retained_if_running'] = process.poll() is None
                save()
                raise
    print(str(report_path))


if __name__ == '__main__':
    main()

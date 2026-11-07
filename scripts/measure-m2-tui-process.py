"""Measure the current TUI in an owned private ConPTY; never use desktop input."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import statistics
import stat
import sys
import time
import uuid

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    'm2_tui_conpty', Path(__file__).with_name('smoke-explorer-conpty.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


class Entry(c.Structure):
    _fields_ = [('size', w.DWORD), ('usage', w.DWORD), ('pid', w.DWORD),
                ('heap', c.c_size_t), ('module', w.DWORD), ('threads', w.DWORD),
                ('parent', w.DWORD), ('priority', w.LONG), ('flags', w.DWORD),
                ('name', w.WCHAR * 260)]


class Memory(c.Structure):
    _fields_ = [('size', w.DWORD), ('faults', w.DWORD)] + [
        (name, c.c_size_t) for name in ('peak_working', 'working', 'peak_pool',
        'pool', 'peak_nonpaged', 'nonpaged', 'pagefile', 'peak_pagefile', 'private')]


def stats(values):
    values = sorted(values)
    return dict(count=len(values), median_ms=statistics.median(values),
                p95_ms=values[math.ceil(.95 * len(values)) - 1], max_ms=max(values))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--host', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    exe, host = [(root / path).resolve(strict=True) for path in (args.exe, args.host)]
    if not exe.is_relative_to(root) or exe.name != 'cide-tui.exe' \
            or not host.is_relative_to(root) or host.name != 'f3_console_host.exe':
        raise RuntimeError('Use the editor and ConPTY host inside this workspace')

    def plain_owned(path, parent, directory=False):
        item = path.lstat()
        if path.parent != parent or path.resolve(strict=True).parent != parent \
                or item.st_file_attributes & 0x400 or stat.S_ISLNK(item.st_mode) \
                or (not stat.S_ISDIR(item.st_mode) if directory else
                    not stat.S_ISREG(item.st_mode) or item.st_nlink != 1):
            raise RuntimeError('Unsafe owned measurement path: ' + str(path))

    work = root / 'work'
    plain_owned(work, root, directory=True)
    folder = work / ('m2-tui-process-' + uuid.uuid4().hex)
    folder.mkdir()
    plain_owned(folder, work, directory=True)
    api = c.WinDLL('kernel32', use_last_error=True)
    api.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
    api.CreateToolhelp32Snapshot.restype = w.HANDLE
    for name in ('Process32FirstW', 'Process32NextW'):
        getattr(api, name).argtypes = [w.HANDLE, c.POINTER(Entry)]
    api.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    api.OpenProcess.restype = w.HANDLE
    api.CloseHandle.argtypes = [w.HANDLE]
    api.GetProcessTimes.argtypes = [w.HANDLE] + [c.POINTER(w.FILETIME)] * 4
    psapi = c.WinDLL('psapi', use_last_error=True)
    psapi.GetProcessMemoryInfo.argtypes = [w.HANDLE, c.POINTER(Memory), w.DWORD]

    def child_handle(parent):
        snapshot = api.CreateToolhelp32Snapshot(2, 0)
        if snapshot == c.c_void_p(-1).value:
            raise c.WinError(c.get_last_error())
        try:
            entry = Entry()
            entry.size = c.sizeof(entry)
            more = api.Process32FirstW(snapshot, c.byref(entry))
            while more:
                if entry.parent == parent and entry.name.lower() == exe.name:
                    handle = api.OpenProcess(0x1000 | 0x10, False, entry.pid)
                    if not handle:
                        raise c.WinError(c.get_last_error())
                    return handle
                more = api.Process32NextW(snapshot, c.byref(entry))
            raise RuntimeError('No owned editor child found')
        finally:
            api.CloseHandle(snapshot)

    def memory(handle):
        item = Memory()
        item.size = c.sizeof(item)
        if not psapi.GetProcessMemoryInfo(handle, c.byref(item), item.size):
            raise c.WinError(c.get_last_error())
        return {key: getattr(item, key) for key in ('private', 'working', 'peak_pagefile')}

    def cpu(handle):
        times = [w.FILETIME() for _ in range(4)]
        if not api.GetProcessTimes(handle, *[c.byref(item) for item in times]):
            raise c.WinError(c.get_last_error())
        return sum((item.dwHighDateTime << 32) + item.dwLowDateTime for item in times[2:]) / 10000

    report = dict(executable=str(exe), sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),
                  desktop_used=False, personal_state_used=False, physical_display_verified=False,
                  scope='Editor process only; private ConPTY host/conhost/parser excluded',
                  unicode_scope='Multibyte narrow glyphs; existing test VT parser does not model wide/combining cells',
                  cases=[])
    output = folder / 'report.json'
    # Create every file exclusively in the freshly admitted directory. Subsequent
    # report updates revalidate it rather than following substituted links.
    with output.open('x', encoding='utf-8') as stream:
        stream.write('{}')
    plain_owned(output, folder)
    for name, unit, size in [('dense', b'abcdefg\n', 4*1024*1024),
                             ('unicode', 'niño é Ελληνικά\r\n'.encode(), 4*1024*1024),
                             ('long', b'x', 16*1024*1024)]:
        source = folder / (name + '.txt')
        original = b'M2_PROCESS_MARKER\r\n' + unit * (size // len(unit))
        plain_owned(folder, work, directory=True)
        with source.open('xb') as stream:
            stream.write(original)
        plain_owned(source, folder)
        session = None
        handle = None
        try:
            started = time.perf_counter()
            session = base.Session(root, exe, host, source, environment=dict(
                os.environ, CIDE_NO_SESSION='1', CIDE_STATE_DIR=str(folder / 'disabled-state')))
            session.resize(240, 40)  # Keep the status visible beside the owned long path.
            session.wait(lambda: session.contains('M2_PROCESS_MARKER'))
            ready = (time.perf_counter() - started)*1000
            handle = child_handle(session.process.pid)
            initial = memory(handle)
            times = []
            # One untimed warmup; every subsequent sample waits for its own status.
            for index in range(64):
                target = 'Lín 1 Col ' + str(2 if index % 2 == 0 else 1) + ' '
                before = time.perf_counter()
                session.send('\x1b[C' if index % 2 == 0 else '\x1b[D')
                session.wait(lambda: session.contains(target))
                if index:
                    times.append((time.perf_counter()-before)*1000)
            for _ in range(8):
                session.send('Q')
                session.wait(lambda: session.contains('QM2_PROCESS_MARKER'))
                session.send('\x1a')
                session.wait(lambda: session.contains('M2_PROCESS_MARKER') and
                             not session.contains('QM2_PROCESS_MARKER'))
            after_edit = memory(handle)
            session.send('\x1b[6~')
            session.wait(lambda: not session.contains('Lín 1 Col 1 '))
            after_page = memory(handle)
            cpu_before = cpu(handle)
            wall = time.perf_counter()
            time.sleep(1)
            idle = dict(cpu_ms=cpu(handle)-cpu_before, wall_ms=(time.perf_counter()-wall)*1000)
            final = memory(handle)
            session.menu('a', 'Salir')
            if session.api.WaitForSingleObject(session.process.process, 5000) != 0:
                raise TimeoutError('Normal owned TUI exit failed')
            code = w.DWORD()
            session.check(session.api.GetExitCodeProcess(session.process.process, c.byref(code)))
            plain_owned(source, folder)
            assert code.value == 0 and source.read_bytes() == original
            report['cases'].append(dict(case=name, bytes=len(original), startup_vt_ms=ready,
                initial=initial, after_edit_undo=after_edit, after_page=after_page, final=final,
                idle=idle, key_to_status_vt=stats(times), samples_ms=times,
                normal_exit=True, fixture_unchanged=True, terminal_cells=[240, 40]))
            print(name + ' complete', flush=True)
        finally:
            if handle:
                api.CloseHandle(handle)
            if session:
                session.close()
            plain_owned(folder, work, directory=True)
            plain_owned(output, folder)
            output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(str(output))


if __name__ == '__main__':
    main()

"""Reinicia sólo procesos GUI propios, ocultos, con depósito y fuente fixture."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import json
import os
from pathlib import Path
import subprocess
import time
import uuid

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe',type=Path,required=True)
    args=parser.parse_args()
    workspace=Path(__file__).resolve().parents[1]
    exe=(workspace/args.exe).resolve(strict=True)
    assert exe.is_relative_to(workspace) and exe.name=='cide-gui.exe'
    inspector=(exe.parent/'examples/recovery_inspect.exe').resolve(strict=True)
    output=workspace/'work/recovery-gui'/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8])
    output.mkdir(parents=True)
    source=output/'documento.rs'
    original=b'fn original() {}\r\n'
    with source.open('xb') as file:file.write(original)
    environment=dict(os.environ,CIDE_NO_SESSION='0',CIDE_STATE_DIR=str(output/'estado'))
    api=c.WinDLL('user32',use_last_error=True)
    callback_type=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
    api.EnumWindows.argtypes=[callback_type,w.LPARAM];api.EnumWindows.restype=w.BOOL
    api.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)];api.GetWindowThreadProcessId.restype=w.DWORD
    api.IsWindowVisible.argtypes=[w.HWND];api.IsWindowVisible.restype=w.BOOL
    api.SendMessageTimeoutW.argtypes=[w.HWND,w.UINT,w.WPARAM,w.LPARAM,w.UINT,w.UINT,c.POINTER(c.c_size_t)];api.SendMessageTimeoutW.restype=w.LPARAM
    api.WaitForInputIdle.argtypes=[w.HANDLE,w.DWORD];api.WaitForInputIdle.restype=w.DWORD
    def owned_window(process):
        found=[]
        @callback_type
        def visit(hwnd,_):
            pid=w.DWORD();api.GetWindowThreadProcessId(hwnd,c.byref(pid))
            if pid.value==process.pid:found.append(hwnd)
            return True
        api.EnumWindows(visit,0)
        return found[0] if found else None
    def send(hwnd,process,message,value=0):
        pid=w.DWORD();api.GetWindowThreadProcessId(hwnd,c.byref(pid))
        assert pid.value==process.pid and process.poll() is None
        result=c.c_size_t()
        if not api.SendMessageTimeoutW(hwnd,message,value,0,0x2,10000,c.byref(result)):
            raise c.WinError(c.get_last_error())
    report=dict(ok=False,output=str(output),processes=0,checks=[],physical_display_verified=False)
    process=None
    try:
        for phase,character in enumerate(['Q','Z',None]):
            startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
            with (output/f'gui-{phase}.stdout.log').open('xb') as stdout,(output/f'gui-{phase}.stderr.log').open('xb') as stderr:
                process=subprocess.Popen([str(exe),'--encoding','utf8',str(source)],cwd=workspace,env=environment,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr,startupinfo=startup)
                if api.WaitForInputIdle(w.HANDLE(int(process._handle)),10000)!=0:raise TimeoutError('La GUI propia no entró al bucle de mensajes')
                deadline=time.monotonic()+10
                hwnd=None
                while hwnd is None and time.monotonic()<deadline:
                    hwnd=owned_window(process)
                    if hwnd is None:time.sleep(.02)
                assert hwnd and not api.IsWindowVisible(hwnd),'La prueba exige HWND propio oculto'
                if character:send(hwnd,process,0x102,ord(character))
                send(hwnd,process,0x10)
                assert process.wait(timeout=15)==0
                process=None
            result=subprocess.run([str(inspector),str(output/'estado/gui')],cwd=workspace,env=environment,capture_output=True,check=True,timeout=15)
            state=json.loads(result.stdout)
            (output/f'restored-{phase}.json').write_text(json.dumps(state,indent=2),encoding='utf8')
            expected=(b'Q' if phase==0 else b'QZ')+original
            assert bytes.fromhex(state['bytes_hex'])==expected
            assert state['dirty'] and state['undo']==min(phase+1,2) and state['cursor']==min(phase+1,2)
            assert state['tabs']==1 and not state['backup']
            assert source.read_bytes()==original
            report['processes']+=1
        report.update(ok=True,original_unchanged=True,checks=['production_gui_hidden_window_own_pid_only','three_processes_restore_unsaved_bytes_original_undo_and_caret','explicit_cli_open_reuses_recovered_document','original_never_written_by_close_or_restore'])
    except BaseException as error:
        report['error']=str(error)
        raise
    finally:
        if process and process.poll() is None:
            process.kill();process.wait(timeout=10)
        report['original_unchanged']=source.read_bytes()==original
        (output/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf8')
        print(json.dumps(report,ensure_ascii=False))
if __name__=='__main__':main()

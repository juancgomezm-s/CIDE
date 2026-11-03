"""Two open sources, immutable comparison and navigation in an owned ConPTY."""
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import importlib.util
import json
from pathlib import Path
import uuid
spec=importlib.util.spec_from_file_location('owned_conpty',Path(__file__).with_name('smoke-explorer-conpty.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
def main():
    root=Path(__file__).resolve().parents[1];parent=root/'work/open-diff-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    left=output/'left.p';right=output/'right.p';fixtures={left:b'same\r\nLEFT_SOURCE\nlast',right:b'same\r\nRIGHT_SOURCE\r\nlast'}
    for p,b in fixtures.items():
        with p.open('xb') as f:f.write(b)
    session=None;checks=[];report={'ok':False,'physical_display_verified':False,'os_clipboard_verified':False,'git_invoked':False}
    def palette(command):
        session.menu('t','Command Palette');session.wait(lambda:session.contains('Command Palette'))
        session.send(command);session.wait(lambda:session.contains(command));session.send('\r')
    try:
        session=base.Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),left)
        session.wait(lambda:session.contains('LEFT_SOURCE'))
        session.menu('v','Split Right');session.wait(lambda:session.contains('Group 2'))
        palette('Focus Group 2')
        session.send('\x0f');session.wait(lambda:session.contains('Open file'));session.send(str(right)+'\r');session.wait(lambda:session.contains('RIGHT_SOURCE'))
        palette('Compare Open Groups');session.wait(lambda:session.contains('Open documents comparison') and session.contains('@@') and session.contains('LEFT_SOURCE') and session.contains('RIGHT_SOURCE'))
        checks.append('open_diff_dual_snapshot_delivers_without_input_or_git')
        session.send('should_not_edit');session.resize(80,24);session.wait(lambda:session.contains('unified') and session.contains('LEFT_SOURCE') and session.contains('RIGHT_SOURCE'))
        session.resize(120,32);session.wait(lambda:session.contains('L | R'))
        checks.append('open_diff_readonly_input_and_compact_restore')
        session.send('\r');session.wait(lambda:session.contains('Comparison hunk selected') and session.contains('Group 2'))
        checks.append('open_diff_enter_navigates_original_right_group')
        palette('Compare Open Groups');session.wait(lambda:session.contains('@@'))
        palette('Open Comparison Hunk in Left Group');session.wait(lambda:session.contains('Comparison hunk selected') and session.contains('Group 1'))
        checks.append('open_diff_palette_navigates_original_left_group')
        palette('Compare Open Groups');session.wait(lambda:session.contains('@@'));session.send('\x1b');session.wait(lambda:not session.contains('@@') and session.contains('Open comparison cancelled'))
        session.menu('f','Exit');assert session.api.WaitForSingleObject(session.process.process,5000)==0
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)));assert code.value==0
        assert all(p.read_bytes()==b for p,b in fixtures.items())
        report.update(ok=True,checks=checks,exit_code=code.value,fixtures_unchanged=True,vt_bytes=session.bytes)
    except BaseException as e:
        report.update(error=str(e),checks=checks,screen=session.text() if session else '');raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf8',newline='\n') as f:json.dump(report,f,indent=2)
        print(json.dumps({'output':str(output),**report}))
if __name__=='__main__':main()

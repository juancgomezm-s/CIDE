"""Final keyboard/overlay flows in an owned hidden ConPTY; no OS clipboard."""
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
    root=Path(__file__).resolve().parents[1];parent=root/'work/tui-closure-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    source=output/'overlay.p';original=b'DISPLAY ORIGINAL.\r\n'
    with source.open('xb') as f:f.write(original)
    session=None;checks=[];report={'ok':False,'avm_started':False,'physical_display_verified':False,'os_clipboard_verified':False}
    try:
        session=base.Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),source)
        session.wait(lambda:session.contains('DISPLAY ORIGINAL.'))
        session.menu('e','Insert Snippet');session.wait(lambda:session.contains('Choose snippet'))
        session.send('Procedure\r');session.wait(lambda:session.contains('Parameter 1/1') and session.contains('newProcedure'))
        # The snippet border surrounds the palette title, so menu_row cannot
        # distinguish that title from the departed Tools menu entry.
        session.send('\x1bt');session.wait(lambda:session.menu_row('Command Palette') is not None)
        session.send('\r');session.wait(lambda:session.contains('Command Palette') and session.contains('Quick Open Project File'))
        session.send('Surround With');session.wait(lambda:session.contains('> Surround With'))
        session.send('\x1b');session.wait(lambda:session.contains('Parameter 1/1') and session.contains('newProcedure') and not session.contains('Command Palette'))
        session.send('\r');session.wait(lambda:session.contains('PROCEDURE newProcedure:'))
        session.send('\x1b');session.wait(lambda:session.contains('Snippet cancelled'))
        assert not session.contains('1:overlay.p*');checks.append('keyboard_targets_palette_above_snippet_without_changing_parameter_or_source')
        session.resize(40,10);session.send('\x1bv');session.wait(lambda:session.menu_row('Show Invisibles') is not None)
        session.send('\x1b[F');session.wait(lambda:session.contains('Open Comparison Hunk'))
        session.send('\x1b[5~');session.send('\x1b[H');session.wait(lambda:session.menu_row('Show Invisibles') is not None)
        session.send('\x1b');assert not session.contains('1:overlay.p*');checks.append('view_menu_home_end_and_page_remain_visible_at_40x10')
        session.resize(120,32);session.send('\x0f');session.wait(lambda:session.contains('Open file'))
        session.send('x'*4097);session.wait(lambda:session.contains('Input exceeds 4096'))
        session.send('\x1b');checks.append('oversize_path_keyboard_input_is_bounded')
        session.menu('f','Exit');assert session.api.WaitForSingleObject(session.process.process,5000)==0
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)));assert code.value==0;assert source.read_bytes()==original
        report.update(ok=True,checks=checks,exit_code=code.value,fixture_unchanged=True,vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error),checks=checks,screen=session.text() if session else '');raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf8',newline='\n') as f:json.dump(report,f,indent=2)
        print(json.dumps({'output':str(output),**report}))
if __name__=='__main__':main()

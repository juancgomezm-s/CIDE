"""Optional local source capabilities in a hidden owned ConPTY; no AVM."""
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
    root=Path(__file__).resolve().parents[1];parent=root/'work/source-services-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    source=output/'source.p';include=output/'common.i';target=output/'target.p'
    fixtures={source:b'PROCEDURE customer:\r\nEND PROCEDURE.\r\n{common.i}\r\nRUN target.p.\r\ncustomer custom\r\ncus',include:b'INCLUDE_TARGET\r\n',target:b'RUN_TARGET\r\n'}
    for p,b in fixtures.items():
        with p.open('xb') as f:f.write(b)
    session=None;checks=[];report={'ok':False,'avm_started':False,'physical_display_verified':False,'os_clipboard_verified':False}
    def palette(command):
        session.menu('t','Command Palette');session.wait(lambda:session.contains('Command Palette'));session.send(command);session.wait(lambda:session.contains(command));session.send('\r')
    try:
        session=base.Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),source)
        session.wait(lambda:session.contains('PROCEDURE customer'))
        session.menu('v','Symbols');session.wait(lambda:session.contains('procedure customer') and session.contains('Symbols (lexical)'))
        session.send('\r');session.wait(lambda:session.contains('Source item accepted') and (session.x,session.y)==(44,3))
        checks.append('source_symbols_deliver_and_navigate_without_input_or_avm')
        session.menu('s','Go to Line / Column');session.wait(lambda:session.contains('Go to line'))
        session.send('6:4\r');session.wait(lambda:(session.x,session.y)==(37,8))
        palette('Complete Local Word');session.wait(lambda:session.contains('core.words: 2') and session.contains('customer (local word)'))
        session.send('not_inserted');session.send('\r');session.wait(lambda:session.contains('1:source.p*') and session.contains('Source item accepted'))
        session.send('\x1a');session.wait(lambda:not session.contains('1:source.p*') and session.contains('cus'))
        checks.append('source_local_completion_explicit_accept_one_undo_without_save')
        session.menu('f','Open Folder');session.wait(lambda:session.contains('Open folder'));session.send(str(output)+'\r');session.wait(lambda:session.contains('Directory metadata ready'))
        session.menu('p','References in Document');session.wait(lambda:session.contains('include common.i') and session.contains('RUN target.p'))
        session.send('\r');session.wait(lambda:session.contains('INCLUDE_TARGET') and session.contains('Group 1: common.i'))
        checks.append('source_static_include_opens_confined_target')
        session.send('\x0f');session.wait(lambda:session.contains('Open file'));session.send(str(source)+'\r');session.wait(lambda:session.contains('PROCEDURE customer'))
        session.menu('p','References in Document');session.wait(lambda:session.contains('RUN target.p'))
        session.send('\x1b[B');palette('Open Source Item to Side');session.wait(lambda:session.contains('RUN_TARGET') and session.contains('Group 2: target.p'))
        checks.append('source_static_run_opens_to_side_without_running_code')
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

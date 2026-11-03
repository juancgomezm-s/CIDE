"""Regex Find/Replace and project flow in an owned hidden Windows ConPTY."""
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
    root=Path(__file__).resolve().parents[1];parent=root/'work/regex-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    source=output/'regex.p';original=b'item12\r\nitem345\r\n'
    with source.open('xb') as f:f.write(original)
    checks=[];session=None;report={'ok':False,'physical_display_verified':False,'os_clipboard_verified':False}
    try:
        session=base.Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),source)
        session.wait(lambda:session.contains('item345'))
        # The resulting overlay itself contains a boxed "Replace" label, so
        # menu disappearance cannot be detected using that label alone.
        session.send('\x1bs');session.wait(lambda:session.menu_row('Replace'))
        session.send('\x1b[B\r');session.wait(lambda:session.contains('Find R:'))
        session.send('item[0-9]+');session.send('\x1br')
        session.wait(lambda:session.contains('R:1') and session.contains('Complete, 2 hits'))
        checks.append('regex_document_completes_without_extra_input')
        session.send('\t$1');session.send('\x1b[20~') # F9 replace all
        session.wait(lambda:session.contains('1:regex.p*') and session.contains('Complete, 0 hits') and not session.contains('item345'))
        checks.append('regex_replace_all_literal_dollars')
        session.send('\x1b');session.send('\x1a')
        session.wait(lambda:session.contains('item345') and not session.contains('1:regex.p*'))
        checks.append('regex_one_undo_restores_source_without_save')
        session.menu('f','Open Folder');session.wait(lambda:session.contains('Open folder'))
        session.send(str(output)+'\r');session.wait(lambda:session.contains('Directory metadata ready'))
        session.menu('s','Find in Project');session.wait(lambda:session.contains('Find in Project: REGEX'))
        session.send('item[0-9]+\r');session.wait(lambda:session.contains('Include globs'))
        session.send('*.p\r');session.wait(lambda:session.contains('Extra exclude'))
        session.send('\r');session.wait(lambda:session.contains('Complete') and session.contains('item12') and session.contains('item345'))
        checks.append('regex_project_results_with_filters')
        session.send('\x1b');session.menu('f','Exit')
        if session.api.WaitForSingleObject(session.process.process,5000)!=0:raise TimeoutError('owned exit deadline')
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)));assert code.value==0
        assert source.read_bytes()==original
        report.update(ok=True,checks=checks,exit_code=code.value,fixture_unchanged=True,vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error),checks=checks,screen=session.text() if session else '');raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf8',newline='\n') as f:json.dump(report,f,indent=2)
        print(json.dumps({'output':str(output),**report}))
if __name__=='__main__':main()

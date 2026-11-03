"""Reviewed snippets and surround in a hidden owned ConPTY, without AVM."""
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
    root=Path(__file__).resolve().parents[1];parent=root/'work/snippets-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir();source=output/'snippet.p';original=b'DISPLAY ORIGINAL.\r\n'
    with source.open('xb') as f:f.write(original)
    session=None;checks=[];report={'ok':False,'avm_started':False,'physical_display_verified':False,'os_clipboard_verified':False}
    def procedure_preview():
        session.menu('e','Insert Snippet');session.wait(lambda:session.contains('Choose snippet'))
        session.send('Procedure\r');session.wait(lambda:session.contains('Parameter 1/1'))
        session.send('\x15demo\r');session.wait(lambda:session.contains('Review snippet') and session.contains('PROCEDURE demo'))
        assert not session.contains('1:snippet.p*')
    try:
        session=base.Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),source)
        session.wait(lambda:session.contains('DISPLAY ORIGINAL.'))
        procedure_preview();session.send('not_source');session.send('\x1b');session.wait(lambda:not session.contains('PROCEDURE demo') and session.contains('Snippet cancelled'))
        assert not session.contains('1:snippet.p*');checks.append('snippet_preview_cancel_and_typing_do_not_edit_source')
        procedure_preview();session.resize(80,24);session.wait(lambda:session.contains('PROCEDURE demo') and session.contains('Review snippet'));session.resize(120,32);session.wait(lambda:session.contains('PROCEDURE demo'))
        checks.append('snippet_review_resize_preserves_unapplied_proposal')
        session.send('\x1b[20~');session.wait(lambda:session.contains('1:snippet.p*') and session.contains('PROCEDURE demo:') and session.contains('DISPLAY ORIGINAL.'))
        session.send('\x1a');session.wait(lambda:not session.contains('PROCEDURE demo') and not session.contains('1:snippet.p*'))
        session.send('\x19');session.wait(lambda:session.contains('PROCEDURE demo') and session.contains('1:snippet.p*'))
        session.send('\x1a');session.wait(lambda:not session.contains('PROCEDURE demo') and not session.contains('1:snippet.p*'))
        checks.append('snippet_explicit_apply_is_one_undo_and_redo_without_save')
        session.send('\x01');session.menu('e','Surround With');session.wait(lambda:session.contains('Choose snippet'))
        session.send('DO block\r');session.wait(lambda:session.contains('Review snippet') and session.contains('+ DO:'))
        assert not session.contains('1:snippet.p*');session.send('\x1b[20~');session.wait(lambda:session.contains('DO:') and session.contains('END.') and session.contains('1:snippet.p*'))
        session.send('\x1a');session.wait(lambda:not session.contains('DO:') and not session.contains('1:snippet.p*'))
        checks.append('snippet_surround_selection_preview_apply_and_atomic_undo')
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

"""Unicode source/caret/rectangle flows in an owned hidden ConPTY, no clipboard."""
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
    root=Path(__file__).resolve().parents[1];parent=root/'work/unicode-conpty';parent.mkdir(exist_ok=True)
    assert parent.resolve().is_relative_to(root) and not parent.is_symlink() and not getattr(parent.lstat(),'st_file_attributes',0)&0x400
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    source=output/'unicode.p';original='\ufeffA e\u0301 界 😀 👨\u200d👩\u200d👧\u200d👦\r\nsecond'.encode('utf8')
    with source.open('xb') as f:f.write(original)
    session=None;checks=[];stage='startup';report={'ok':False,'avm_started':False,'physical_display_verified':False,'os_clipboard_verified':False}
    def goto(position):
        session.menu('s','Go to Line / Column');session.wait(lambda:session.contains('Go to line'))
        session.send(position+'\r')
    try:
        session=base.Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),source)
        session.wait(lambda:session.contains('second'));stage='goto origin';goto('1:1');session.wait(lambda:(session.x,session.y)==(34,3))
        for column in [1,2,3,4,6,7,9,10,12]:
            stage='right to visual '+str(column)
            session.send('\x1b[C');session.wait(lambda:(session.x,session.y)==(34+column,3))
        session.send('\x1b[C');session.wait(lambda:(session.x,session.y)==(34,4))
        session.send('\x1b[D');session.wait(lambda:(session.x,session.y)==(46,3))
        checks.append('unicode_combining_cjk_emoji_zwj_cursor_and_atomic_crlf')
        stage='backspace family';session.send('\x7f');session.wait(lambda:(session.x,session.y)==(44,3) and session.contains('1:unicode.p*'))
        session.send('\x1a');session.wait(lambda:not session.contains('1:unicode.p*'))
        checks.append('unicode_family_backspace_is_one_undo_without_save')
        stage='rectangle origin';goto('1:6');session.wait(lambda:(session.x,session.y)==(38,3))
        session.menu('e','Rectangle: Select From Caret/Selection');session.menu('e','Rectangle: Extend Right')
        session.send('x');session.wait(lambda:session.contains('1:unicode.p*'))
        session.send('\x1a');session.wait(lambda:not session.contains('1:unicode.p*'))
        checks.append('unicode_partial_wide_rectangle_replace_one_undo')
        session.resize(40,10);session.wait(lambda:session.width==40);session.resize(120,32)
        goto('1:1');session.wait(lambda:(session.x,session.y)==(34,3))
        checks.append('unicode_narrow_resize_preserves_source_and_legal_caret')
        session.menu('f','Exit');assert session.api.WaitForSingleObject(session.process.process,5000)==0
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)));assert code.value==0;assert source.read_bytes()==original
        report.update(ok=True,checks=checks,exit_code=code.value,fixture_unchanged=True,vt_bytes=session.bytes)
    except BaseException as error:
        report.update(error=str(error),checks=checks,screen=session.text() if session else '',stage=stage,cursor=[session.x,session.y] if session else None);raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf8',newline='\n') as f:json.dump(report,f,indent=2)
        print(json.dumps({'output':str(output),**report}))
if __name__=='__main__':main()

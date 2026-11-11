"""Syntax flow in our own hidden ConPTY; no physical display/clipboard claim."""
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import sys
import uuid

spec=importlib.util.spec_from_file_location('owned_conpty',Path(__file__).with_name('smoke-explorer-conpty.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
class Session(base.Session):
    def __init__(self,*args,**kwargs):
        self.foreground=None;self.colors=[[None]*120 for _ in range(32)]
        super().__init__(*args,**kwargs)
    def clear(self):
        super().clear();self.colors=[[None]*self.width for _ in range(self.height)]
    def feed(self,ch):
        if self.escape=='csi' and ch=='m':
            parts=[int(p) if p.isdigit() else 0 for p in self.sequence.split(';')]
            at=0
            while at<len(parts):
                value=parts[at]
                if value in (0,39):self.foreground=None
                elif value==38 and at+4<len(parts) and parts[at+1]==2:
                    self.foreground=tuple(parts[at+2:at+5]);at+=4
                at+=1
        printable=not self.escape and ch>=' ' and ch!='\x7f'
        x,y=self.x,self.y
        if printable and x>=self.width:x=0;y=min(self.height-1,y+1)
        super().feed(ch)
        if printable and 0<=x<self.width and 0<=y<self.height:self.colors[y][x]=self.foreground
    def word_color(self,text,color):
        for row in range(2,self.height-1):
            line=''.join(self.cells[row]);at=line.find(text)
            if at>=0 and self.colors[row][at]==color:return True
        return False

def main():
    if sys.platform!='win32' or c.sizeof(c.c_void_p)!=8:raise RuntimeError('Windows x64 only')
    root=Path(__file__).resolve().parents[1];parent=root/'work/syntax-conpty';parent.mkdir(exist_ok=True)
    if parent.is_symlink() or getattr(parent.lstat(),'st_file_attributes',0)&0x400 or not parent.resolve().is_relative_to(root):raise RuntimeError('owned output path escaped workspace')
    output=parent/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    source=output/'syntax.p';data=output/'data.json';rust=output/'module.rs';plain=output/'fallback.py'
    original=('\ufeffDEFINE VARIABLE n AS INTEGER NO-UNDO.\r\n/* outer\r\n'+'inside comment\r\n'*60+'*/\r\nDISPLAY "color" 42.\r\n').encode('utf8')
    fixtures={source:original,data:b'{ "name": "color", "enabled": true, "number": 42 }\r\n',rust:b'fn borrow<\'a>(x: &\'a str) {\r\nlet raw = r##"first\r\nsecond"##;\r\nlet ch = \'x\'; let n = 0xff_u8;\r\n/* outer /* inner */ keep */\r\n}\r\n',plain:b'DEFINE 42\r\n'}
    for path,value in fixtures.items():
        with path.open('xb') as stream:stream.write(value)
    session=None;checks=[];payload={'ok':False,'physical_display_verified':False,'os_clipboard_verified':False,'output':str(output)}
    try:
        child_environment=os.environ.copy();child_environment.pop('NO_COLOR',None)
        session=Session(root,(root/'target/release/cide-tui.exe').resolve(strict=True),(root/'target/release/examples/f3_console_host.exe').resolve(strict=True),source,environment=child_environment)
        pink=(249,38,114);yellow=(230,219,116);comment=(138,135,117);purple=(174,129,255)
        session.wait(lambda:session.word_color('DEFINE',pink) and session.word_color('inside',comment))
        checks.append('syntax_conpty_abl_keyword_multiline_comment_rgb')
        # Win32 console input delivers these as two key events; do not assume
        # bracketed paste produces the single Event::Paste used by model tests.
        session.send('/*');session.wait(lambda:session.word_color('DEFINE',comment))
        session.send('\x1a'*2);session.wait(lambda:session.word_color('DEFINE',pink) and not session.contains('1:syntax.p*'))
        session.send('\x19'*2);session.wait(lambda:session.word_color('DEFINE',comment))
        session.send('\x1a'*2);session.wait(lambda:session.word_color('DEFINE',pink) and not session.contains('1:syntax.p*'))
        checks.append('syntax_conpty_insert_undo_redo_recolors_without_save')
        session.send('\x1b[6~'*7);session.wait(lambda:session.word_color('DISPLAY',pink) and session.word_color('color',yellow) and session.word_color('42',purple))
        checks.append('syntax_conpty_scroll_recovers_context_and_string_number')
        session.send('\x0f');session.wait(lambda:session.contains('Open file'))
        session.send(str(data)+'\r');session.wait(lambda:session.word_color('color',yellow) and session.word_color('true',pink))
        session.send('\x0f');session.wait(lambda:session.contains('Open file'))
        session.send(str(rust)+'\r');session.wait(lambda:session.word_color('fn',pink) and session.word_color('second',yellow) and session.word_color('keep',comment) and session.word_color('0xff_u8',purple) and session.word_color("'a",(102,217,239)) and session.word_color("'x'",yellow))
        checks.append('syntax_conpty_rust_raw_nested_comments_lifetime_character')
        session.send('\x0f');session.wait(lambda:session.contains('Open file'))
        session.send(str(plain)+'\r');session.wait(lambda:session.word_color('DEFINE',None))
        session.send('\x1b[5;5~');session.wait(lambda:session.word_color('fn',pink))
        session.send('\x1b[5;5~');session.wait(lambda:session.word_color('true',pink))
        checks.append('syntax_conpty_profile_switch_json_plain_and_return')
        session.resize(80,24);session.wait(lambda:session.word_color('true',pink))
        session.resize(120,32);session.wait(lambda:session.word_color('true',pink))
        session.menu('f','Exit')
        if session.api.WaitForSingleObject(session.process.process,5000)!=0:raise TimeoutError('owned terminal exit deadline')
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)))
        if code.value:raise RuntimeError('owned terminal exit '+str(code.value))
        assert all(path.read_bytes()==value for path,value in fixtures.items())
        payload.update(ok=True,checks=checks,exit_code=code.value,fixtures_unchanged=True,vt_bytes=session.bytes,screen=session.text())
    except BaseException as error:
        payload.update(error=str(error),checks=checks,screen=session.text() if session else '');raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf8',newline='\n') as stream:json.dump(payload,stream,ensure_ascii=True,indent=2)
        print(json.dumps({key:value for key,value in payload.items() if key!='screen'}))
if __name__=='__main__':main()

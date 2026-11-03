"""Own hidden ConPTY functional Explorer flow; no latency/RAM benchmark or .NET."""
import codecs
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import uuid

class Coord(c.Structure):
    _fields_=[('x',c.c_short),('y',c.c_short)]
class Startup(c.Structure):
    _fields_=[('cb',w.DWORD),('reserved',w.LPWSTR),('desktop',w.LPWSTR),('title',w.LPWSTR),
        ('x',w.DWORD),('y',w.DWORD),('cx',w.DWORD),('cy',w.DWORD),('charsx',w.DWORD),('charsy',w.DWORD),
        ('fill',w.DWORD),('flags',w.DWORD),('show',w.WORD),('cbreserved',w.WORD),('preserved',c.c_void_p),
        ('input',w.HANDLE),('output',w.HANDLE),('error',w.HANDLE)]
class StartupEx(c.Structure):
    _fields_=[('startup',Startup),('attributes',c.c_void_p)]
class Process(c.Structure):
    _fields_=[('process',w.HANDLE),('thread',w.HANDLE),('pid',w.DWORD),('tid',w.DWORD)]

class Session:
    def __init__(self,root,exe,host,source):
        self.api=c.WinDLL('kernel32',use_last_error=True)
        signatures={
            'CreatePipe':([c.POINTER(w.HANDLE),c.POINTER(w.HANDLE),c.c_void_p,w.DWORD],w.BOOL),
            'CreatePseudoConsole':([Coord,w.HANDLE,w.HANDLE,w.DWORD,c.POINTER(w.HANDLE)],c.c_long),
            'ResizePseudoConsole':([w.HANDLE,Coord],c.c_long),'ClosePseudoConsole':([w.HANDLE],None),
            'InitializeProcThreadAttributeList':([c.c_void_p,w.DWORD,w.DWORD,c.POINTER(c.c_size_t)],w.BOOL),
            'UpdateProcThreadAttribute':([c.c_void_p,w.DWORD,c.c_size_t,c.c_void_p,c.c_size_t,c.c_void_p,c.c_void_p],w.BOOL),
            'DeleteProcThreadAttributeList':([c.c_void_p],None),
            'CreateProcessW':([w.LPCWSTR,w.LPWSTR,c.c_void_p,c.c_void_p,w.BOOL,w.DWORD,c.c_void_p,w.LPCWSTR,c.POINTER(StartupEx),c.POINTER(Process)],w.BOOL),
            'ReadFile':([w.HANDLE,c.c_void_p,w.DWORD,c.POINTER(w.DWORD),c.c_void_p],w.BOOL),
            'WriteFile':([w.HANDLE,c.c_void_p,w.DWORD,c.POINTER(w.DWORD),c.c_void_p],w.BOOL),
            'CloseHandle':([w.HANDLE],w.BOOL),'WaitForSingleObject':([w.HANDLE,w.DWORD],w.DWORD),
            'GetExitCodeProcess':([w.HANDLE,c.POINTER(w.DWORD)],w.BOOL),
            'CreateJobObjectW':([c.c_void_p,w.LPCWSTR],w.HANDLE),
            'SetInformationJobObject':([w.HANDLE,c.c_int,c.c_void_p,w.DWORD],w.BOOL),
        }
        for name,(args,result) in signatures.items():
            fn=getattr(self.api,name);fn.argtypes=args;fn.restype=result
        self.handles=[];self.console=None;self.job=None;self.attributes=None;self.process=Process();self.reader=None
        self.sync=threading.Condition();self.send_lock=threading.Lock();self.error=None;self.bytes=0;self.raw_limit=2*1024*1024
        self.width=120;self.height=32;self.x=0;self.y=0;self.saved=(0,0);self.escape='';self.reverse=False;self.sequence=''
        self.cells=[[' ']*self.width for _ in range(self.height)];self.styles=[[False]*self.width for _ in range(self.height)]
        try:
            iread,iwrite,oread,owrite=[w.HANDLE() for _ in range(4)]
            self.check(self.api.CreatePipe(c.byref(iread),c.byref(iwrite),None,0));self.handles += [iread,iwrite]
            self.check(self.api.CreatePipe(c.byref(oread),c.byref(owrite),None,0));self.handles += [oread,owrite]
            self.input=iwrite;self.output=oread;console=w.HANDLE()
            if self.api.CreatePseudoConsole(Coord(self.width,self.height),iread,owrite,0,c.byref(console))<0:raise OSError('CreatePseudoConsole failed')
            self.console=console
            self.job=self.api.CreateJobObjectW(None,None);self.check(self.job)
            limits=c.create_string_buffer(144);c.cast(c.byref(limits,16),c.POINTER(w.DWORD))[0]=0x2000
            self.check(self.api.SetInformationJobObject(self.job,9,limits,len(limits)))
            size=c.c_size_t();self.api.InitializeProcThreadAttributeList(None,2,0,c.byref(size))
            self.attributes=c.create_string_buffer(size.value)
            self.check(self.api.InitializeProcThreadAttributeList(self.attributes,2,0,c.byref(size)))
            self.check(self.api.UpdateProcThreadAttribute(self.attributes,0,0x00020016,console,c.sizeof(w.HANDLE),None,None))
            jobs=(w.HANDLE*1)(self.job)
            self.check(self.api.UpdateProcThreadAttribute(self.attributes,0,0x0002000D,jobs,c.sizeof(jobs),None,None))
            startup=StartupEx();startup.startup.cb=c.sizeof(StartupEx);startup.attributes=c.cast(self.attributes,c.c_void_p)
            command=c.create_unicode_buffer(subprocess.list2cmdline([str(host),str(exe),'--encoding','utf8',str(source)]))
            self.check(self.api.CreateProcessW(str(host),command,None,None,False,0x80000,None,str(root),c.byref(startup),c.byref(self.process)))
            for handle in (iread,owrite):self.api.CloseHandle(handle);self.handles.remove(handle)
            self.reader=threading.Thread(target=self.read,name='cide-own-conpty');self.reader.start()
        except BaseException:
            self.close();raise
    def check(self,value):
        if not value:raise c.WinError(c.get_last_error())
    def send(self,text):
        data=text.encode('utf-8');written=w.DWORD()
        with self.send_lock:
            self.check(self.api.WriteFile(self.input,data,len(data),c.byref(written),None))
            if written.value!=len(data):raise OSError('partial owned ConPTY input')
    def clear(self):
        self.cells=[[' ']*self.width for _ in range(self.height)];self.styles=[[False]*self.width for _ in range(self.height)]
    def feed(self,ch):
        if self.escape=='osc':
            if ch=='\x07':self.escape=''
            elif ch=='\x1b':self.escape='osc-end'
            return
        if self.escape=='osc-end':self.escape='' if ch=='\\' else 'osc';return
        if self.escape=='esc':
            self.escape='csi' if ch=='[' else 'osc' if ch==']' else '';self.sequence=''
            if ch=='7':self.saved=(self.x,self.y)
            if ch=='8':self.x,self.y=self.saved
            return
        if self.escape=='csi':
            if not ('@'<=ch<='~'):
                self.sequence+=ch
                if len(self.sequence)>128:raise RuntimeError('VT sequence budget')
                return
            body=self.sequence;self.escape='';parts=body.split(';')
            number=int(parts[0]) if parts[0].isdigit() and int(parts[0]) else 1
            if ch in 'Hf':self.y=max(0,min(self.height-1,number-1));self.x=max(0,min(self.width-1,(int(parts[1]) if len(parts)>1 and parts[1].isdigit() else 1)-1))
            elif ch=='A':self.y=max(0,self.y-number)
            elif ch=='B':self.y=min(self.height-1,self.y+number)
            elif ch=='C':self.x=min(self.width-1,self.x+number)
            elif ch=='D':self.x=max(0,self.x-number)
            elif ch=='G':self.x=min(self.width-1,number-1)
            elif ch=='d':self.y=min(self.height-1,number-1)
            elif ch=='J' and body=='2':self.clear()
            elif ch=='K':
                mode=int(body) if body.isdigit() else 0
                for x in range(self.width):
                    if mode==2 or mode==0 and x>=self.x or mode==1 and x<=self.x:self.cells[self.y][x]=' ';self.styles[self.y][x]=False
            elif ch=='X':
                for x in range(self.x,min(self.width,self.x+number)):self.cells[self.y][x]=' ';self.styles[self.y][x]=False
            elif ch=='m':
                for part in parts:
                    if part in ('','0','27'):self.reverse=False
                    elif part=='7':self.reverse=True
            elif ch=='n' and body=='6':self.send('\x1b[1;1R')
            elif ch=='s':self.saved=(self.x,self.y)
            elif ch=='u':self.x,self.y=self.saved
            return
        if ch=='\x1b':self.escape='esc';return
        if ch=='\r':self.x=0;return
        if ch=='\n':self.y=min(self.height-1,self.y+1);return
        if ch<' ':return
        if self.x>=self.width:self.x=0;self.y=min(self.height-1,self.y+1)
        self.cells[self.y][self.x]=ch;self.styles[self.y][self.x]=self.reverse;self.x+=1
    def read(self):
        decoder=codecs.getincrementaldecoder('utf-8')('strict');data=c.create_string_buffer(8192);count=w.DWORD()
        try:
            while self.api.ReadFile(self.output,data,len(data),c.byref(count),None) and count.value:
                with self.sync:
                    self.bytes+=count.value
                    if self.bytes>self.raw_limit:raise RuntimeError('owned ConPTY output budget')
                    for ch in decoder.decode(data.raw[:count.value]):self.feed(ch)
                    self.sync.notify_all()
        except BaseException as error:
            with self.sync:self.error=str(error);self.sync.notify_all()
    def text(self):return '\n'.join(''.join(row) for row in self.cells)
    def wait(self,predicate,timeout=8):
        deadline=time.monotonic()+timeout
        with self.sync:
            while not predicate():
                if self.error:raise RuntimeError(self.error)
                left=deadline-time.monotonic()
                if left<=0:raise TimeoutError('owned ConPTY state not reached\n'+self.text())
                self.sync.wait(min(left,0.1))
    def contains(self,text):return text in self.text()
    def tree_contains(self,label):
        return any(label in ''.join(row).split('\u2502',1)[0] for row in self.cells[4:-1])
    def menu_row(self,label,selected=False):
        for row,styles in zip(self.cells,self.styles):
            text=''.join(row);at=text.find(label)
            if at>=0 and '\u2502' in text[:at] and '\u2502' in text[at+len(label):]:
                if not selected or any(styles[at:at+len(label)]):return True
        return False
    def menu(self,letter,label):
        self.send('\x1b'+letter);self.wait(lambda:self.menu_row(label))
        for _ in range(24):
            with self.sync:
                if self.menu_row(label,selected=True):break
                old=[tuple(row) for row in self.styles]
            self.send('\x1b[B');self.wait(lambda:old!=[tuple(row) for row in self.styles])
        else:raise RuntimeError('menu item not selected: '+label)
        self.send('\r')
        self.wait(lambda:not self.menu_row(label))
    def resize(self,width,height):
        with self.sync:
            self.width,self.height=width,height;self.x=self.y=0;self.clear()
            if self.api.ResizePseudoConsole(self.console,Coord(width,height))<0:raise OSError('own ConPTY resize failed')
    def close(self):
        if self.job:self.api.CloseHandle(self.job);self.job=None
        if self.process.process:self.api.WaitForSingleObject(self.process.process,3000)
        if self.console:self.api.ClosePseudoConsole(self.console);self.console=None
        for handle in self.handles:self.api.CloseHandle(handle)
        self.handles=[]
        if self.reader:
            self.reader.join(5)
            if self.reader.is_alive():raise RuntimeError('owned ConPTY reader did not finish')
        for handle in (self.process.process,self.process.thread):
            if handle:self.api.CloseHandle(handle)
        if self.attributes:self.api.DeleteProcThreadAttributeList(self.attributes);self.attributes=None

def main():
    if sys.platform!='win32' or c.sizeof(c.c_void_p)!=8:raise RuntimeError('Windows x64 only')
    root=Path(__file__).resolve().parents[1];base=root
    for name in ('work','explorer-conpty'):
        base=base/name;base.mkdir(exist_ok=True)
        if base.is_symlink() or getattr(base.lstat(),'st_file_attributes',0)&0x400 or not base.resolve().is_relative_to(root):raise RuntimeError('unsafe owned ConPTY output directory')
    output=base/(dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8]);output.mkdir()
    fixture=output/'project';fixture.mkdir();(fixture/'folder').mkdir()
    source=fixture/'source.txt';nested=fixture/'folder/nested.txt';original=b'EXPLORER_SOURCE\r\nsecond\r\n'
    for path,value in [(source,original),(nested,b'NESTED_EXPLORER\r\n')]:
        with path.open('xb') as stream:stream.write(value)
    exe=(root/'target/release/cide-tui.exe').resolve(strict=True);host=(root/'target/release/examples/f3_console_host.exe').resolve(strict=True)
    session=None;checks=[];payload={'ok':False,'physical_display_verified':False,'os_clipboard_verified':False,'output':str(output)}
    try:
        session=Session(root,exe,host,source)
        session.wait(lambda:session.contains('EXPLORER_SOURCE'))
        session.menu('f','Open Folder');session.wait(lambda:session.contains('Open folder'))
        session.send(str(fixture)+'\r');session.wait(lambda:session.contains('Directory metadata ready') and session.contains('[+] folder'))
        checks.append('terminal_explorer_delivered_without_another_key')
        session.menu('v','Explorer');session.wait(lambda:session.contains('Explorer'))
        session.send('\x1b[H\x1b[B\x1b[C');session.wait(lambda:session.tree_contains('nested.txt'))
        checks.append('terminal_explorer_lazy_expand_keyboard')
        session.send('\x1b[B\r');session.wait(lambda:session.contains('NESTED_EXPLORER'))
        checks.append('terminal_explorer_confined_open')
        session.send('Q');session.wait(lambda:session.contains('QNESTED_EXPLORER'))
        session.menu('v','Explorer');session.send('\x1b[H\x1b[15~\x1b')
        session.wait(lambda:session.contains('Explorer: editor focus'))
        checks.append('terminal_explorer_refresh_cancel_preserves_source')
        session.menu('v','Explorer');session.send('\x1b[15~')
        session.wait(lambda:session.contains('Explorer refresh requested'))
        session.wait(lambda:session.contains('Directory metadata ready') and session.contains('[+] folder'))
        session.send('\x1b[H\x1b[B\x1b[C');session.wait(lambda:session.tree_contains('nested.txt'))
        session.send('\x1b[B\r')
        session.wait(lambda:session.contains('QNESTED_EXPLORER'))
        session.send('\x1a');session.wait(lambda:session.contains('NESTED_EXPLORER') and not session.contains('QNESTED_EXPLORER'))
        checks.append('terminal_explorer_dirty_reuse_shared_undo_without_save')
        session.resize(80,24);session.wait(lambda:session.contains('NESTED_EXPLORER'))
        session.resize(120,32);session.wait(lambda:session.contains('NESTED_EXPLORER'))
        checks.append('terminal_explorer_resize_preserves_document')
        session.menu('f','Exit')
        if session.api.WaitForSingleObject(session.process.process,5000)!=0:raise TimeoutError('owned terminal exit deadline')
        code=w.DWORD();session.check(session.api.GetExitCodeProcess(session.process.process,c.byref(code)))
        if code.value:raise RuntimeError('owned terminal exit '+str(code.value))
        assert source.read_bytes()==original and nested.read_bytes()==b'NESTED_EXPLORER\r\n'
        payload.update(ok=True,checks=checks,exit_code=code.value,fixtures_unchanged=True,vt_bytes=session.bytes,screen=session.text())
    except BaseException as error:
        payload.update(error=str(error),checks=checks,screen=session.text() if session else '')
        raise
    finally:
        if session:session.close()
        with (output/'report.json').open('x',encoding='utf-8',newline='\n') as stream:json.dump(payload,stream,ensure_ascii=True,indent=2)
        print(json.dumps({k:v for k,v in payload.items() if k!='screen'}))

if __name__=='__main__':main()

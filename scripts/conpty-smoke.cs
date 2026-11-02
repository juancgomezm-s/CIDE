// Windows-only test host. Launches only the supplied JC-IDE executable, in a hidden pseudoconsole.
using System;
using System.IO;
using System.Text;
using System.Diagnostics;
using System.Threading;
using System.Runtime.InteropServices;
using System.ComponentModel;
using System.Collections.Generic;

public static class JcConPtySmoke {
    [StructLayout(LayoutKind.Sequential)] struct Coord {public short X,Y; public Coord(short x,short y){X=x;Y=y;}}
    [StructLayout(LayoutKind.Sequential)] struct Startup {
        public int cb;public IntPtr reserved,desktop,title;public int x,y,xSize,ySize,xChars,yChars,fill,flags;
        public short show,reservedBytes;public IntPtr reservedPtr,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)] struct StartupEx {public Startup startup;public IntPtr attributes;}
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo {public IntPtr process,thread;public int processId,threadId;}
    [DllImport("kernel32",SetLastError=true)] static extern bool CreatePipe(out IntPtr read,out IntPtr write,IntPtr security,int size);
    [DllImport("kernel32")] static extern int CreatePseudoConsole(Coord size,IntPtr input,IntPtr output,int flags,out IntPtr console);
    [DllImport("kernel32")] static extern int ResizePseudoConsole(IntPtr console,Coord size);
    [DllImport("kernel32")] static extern void ClosePseudoConsole(IntPtr console);
    [DllImport("kernel32",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr bytes);
    [DllImport("kernel32",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,int flags,IntPtr attribute,IntPtr value,IntPtr bytes,IntPtr previous,IntPtr returned);
    [DllImport("kernel32")] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string app,StringBuilder command,IntPtr processAttributes,IntPtr threadAttributes,bool inherit,int flags,IntPtr environment,string directory,ref StartupEx startup,out ProcessInfo process);
    [DllImport("kernel32",SetLastError=true)] static extern bool ReadFile(IntPtr file,byte[] bytes,int length,out int read,IntPtr overlapped);
    [DllImport("kernel32",SetLastError=true)] static extern bool WriteFile(IntPtr file,byte[] bytes,int length,out int written,IntPtr overlapped);
    [DllImport("kernel32")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32")] static extern int WaitForSingleObject(IntPtr handle,int milliseconds);
    [DllImport("kernel32")] static extern bool GetExitCodeProcess(IntPtr process,out int code);
    [DllImport("kernel32")] static extern bool TerminateProcess(IntPtr process,int code);
    [DllImport("kernel32",SetLastError=true)] static extern IntPtr OpenProcess(int access,bool inherit,int pid);
    static void Check(bool result){if(!result)throw new Win32Exception(Marshal.GetLastWin32Error());}
    static void HResult(int code){if(code!=0)Marshal.ThrowExceptionForHR(code);}
    static string Quote(string arg){return "\""+arg.Replace("\"","\\\"")+"\"";}

    sealed class Display {
        internal readonly object sync=new object();internal int width=80,height=24,x,y,clears,childPid;internal long bytes;
        readonly StringBuilder marker=new StringBuilder();
        char[,] cells=new char[24,80];StringBuilder sequence=new StringBuilder();StringBuilder raw=new StringBuilder();int escape=0,savedX,savedY;readonly IntPtr input;readonly object sendLock=new object();
        internal Display(IntPtr input){this.input=input;Clear();}
        void Clear(){for(int r=0;r<height;r++)for(int c=0;c<width;c++)cells[r,c]=' ';x=0;y=0;}
        internal void Send(string text){byte[] data=Encoding.UTF8.GetBytes(text);lock(sendLock){int written;Check(WriteFile(input,data,data.Length,out written,IntPtr.Zero));if(written!=data.Length)throw new IOException("Partial test input");}}
        void Char(char ch){
            if(childPid==0 && marker.Length<256){marker.Append(ch);string value=marker.ToString();int begin=value.IndexOf("JC_CHILD_PID=");int end=value.IndexOf(';');if(begin>=0 && end>begin+13)int.TryParse(value.Substring(begin+13,end-begin-13),out childPid);}
            if(escape==3){if(ch=='\x07')escape=0;else if(ch=='\x1b')escape=4;return;}
            if(escape==4){escape=ch=='\\'?0:3;return;}
            if(escape==1){if(ch=='['){escape=2;sequence.Clear();}else if(ch==']')escape=3;else {escape=0;if(ch=='7'){savedX=x;savedY=y;}if(ch=='8'){x=savedX;y=savedY;}}return;}
            if(escape==2){
                if(ch>='@' && ch<='~'){
                    string body=sequence.ToString();escape=0;int number;if(!int.TryParse(body,out number)||number==0)number=1;
                    if(ch=='H'||ch=='f'){string[] p=body.Split(';');int row=1,col=1;if(p.Length>0 && p[0].Length>0)int.TryParse(p[0],out row);if(p.Length>1)int.TryParse(p[1],out col);y=Math.Max(0,Math.Min(height-1,row-1));x=Math.Max(0,Math.Min(width-1,col-1));}
                    else if(ch=='J'&&body=="2"){Clear();clears++;}
                    else if(ch=='A')y=Math.Max(0,y-number);
                    else if(ch=='B')y=Math.Min(height-1,y+number);
                    else if(ch=='C')x=Math.Min(width-1,x+number);
                    else if(ch=='D')x=Math.Max(0,x-number);
                    else if(ch=='G')x=Math.Min(width-1,number-1);
                    else if(ch=='d')y=Math.Min(height-1,number-1);
                    else if(ch=='K'){int mode=0;int.TryParse(body,out mode);for(int c=0;c<width;c++)if(mode==2 || mode==0&&c>=x || mode==1&&c<=x)cells[y,c]=' ';}
                    else if(ch=='X'){for(int c=x;c<Math.Min(width,x+number);c++)cells[y,c]=' ';}
                    else if(ch=='s'){savedX=x;savedY=y;}
                    else if(ch=='u'){x=savedX;y=savedY;}
                    else if(ch=='n'&&body=="6")Send("\x1b[1;1R");
                }else if(sequence.Length<256)sequence.Append(ch);else escape=0;
                return;
            }
            if(ch=='\x1b'){escape=1;return;}if(ch=='\r'){x=0;return;}if(ch=='\n'){y=Math.Min(height-1,y+1);return;}if(ch=='\b'){x=Math.Max(0,x-1);return;}
            if(ch<32)return;if(x>=width){x=0;y=Math.Min(height-1,y+1);}cells[y,x++]=ch;
        }
        internal string Text(){StringBuilder value=new StringBuilder();for(int row=0;row<height;row++){for(int col=0;col<width;col++)value.Append(cells[row,col]);value.Append('\n');}return value.ToString();}
        internal void Resize(short w,short h){lock(sync){width=w;height=h;cells=new char[h,w];Clear();}}
        internal void Read(IntPtr pipe){
            byte[] data=new byte[8192];char[] chars=new char[8192];Decoder decoder=Encoding.UTF8.GetDecoder();int read;
            while(ReadFile(pipe,data,data.Length,out read,IntPtr.Zero)&&read>0){lock(sync){bytes+=read;if(raw.Length<4096)raw.Append(BitConverter.ToString(data,0,Math.Min(read,1024)));int count=decoder.GetChars(data,0,read,chars,0);for(int i=0;i<count;i++)Char(chars[i]);Monitor.PulseAll(sync);}}
        }
        internal void Wait(Func<bool> condition,int timeout){Stopwatch clock=Stopwatch.StartNew();lock(sync){while(!condition()){int left=timeout-(int)clock.ElapsedMilliseconds;if(left<=0)throw new TimeoutException("ConPTY state not observed; bytes="+bytes+"\n"+Text());Monitor.Wait(sync,left);}}}
    }
    public sealed class Result {
        public int samples;public double injection_to_vt_p50_ms,injection_to_vt_p95_ms;
        public long private_bytes,keyboard_output_bytes;public double idle_cpu_ms;public int clears_during_keys,resize_clears,exit_code;
        public bool project_search_delivered;
        public bool project_replace_previewed,project_replace_applied_and_undone;
        public bool document_search_folded_without_more_input,document_goto_scalars,document_replaced_and_undone;
    }
    public static Result Run(string executable,string host,string directory,bool project,bool document){
        string root=Path.GetFullPath(directory);string relative=project ? "work/s1-smoke-"+Process.GetCurrentProcess().Id+"/source.txt" : "work/f3-smoke-"+Process.GetCurrentProcess().Id+".txt";string path=Path.GetFullPath(Path.Combine(root,relative));
        if(!path.StartsWith(root.TrimEnd(Path.DirectorySeparatorChar)+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase))throw new IOException("Test path outside workspace");
        bool created=false;string fixtureDir=null;IntPtr inputRead=IntPtr.Zero,inputWrite=IntPtr.Zero,outputRead=IntPtr.Zero,outputWrite=IntPtr.Zero,console=IntPtr.Zero,attributes=IntPtr.Zero,editor=IntPtr.Zero;ProcessInfo child=new ProcessInfo();Thread reader=null;
        try{
            if(project){string candidate=Path.GetDirectoryName(path);if(Directory.Exists(candidate))throw new IOException("Test directory already exists");Directory.CreateDirectory(candidate);fixtureDir=candidate;}
            using(FileStream file=new FileStream(path,FileMode.CreateNew,FileAccess.Write)){byte[] content=Encoding.UTF8.GetBytes("F3_VISIBLE_"+new string('x',128)+(document ? "\r\nStraße NIÑO\r\n" : "\nnext\n"));file.Write(content,0,content.Length);created=true;}
            Check(CreatePipe(out inputRead,out inputWrite,IntPtr.Zero,0));Check(CreatePipe(out outputRead,out outputWrite,IntPtr.Zero,0));HResult(CreatePseudoConsole(new Coord(80,24),inputRead,outputWrite,0,out console));
            CloseHandle(inputRead);inputRead=IntPtr.Zero;CloseHandle(outputWrite);outputWrite=IntPtr.Zero;
            IntPtr bytes=IntPtr.Zero;InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref bytes);attributes=Marshal.AllocHGlobal(bytes);Check(InitializeProcThreadAttributeList(attributes,1,0,ref bytes));
            Check(UpdateProcThreadAttribute(attributes,0,new IntPtr(0x00020016),console,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero));
            StartupEx startup=new StartupEx();startup.startup.cb=Marshal.SizeOf(typeof(StartupEx));startup.attributes=attributes;
            Check(CreateProcessW(host,new StringBuilder(Quote(host)+" "+Quote(executable)+" --encoding utf8 "+Quote(relative)),IntPtr.Zero,IntPtr.Zero,false,0x00080000,IntPtr.Zero,root,ref startup,out child));
            Display display=new Display(inputWrite);IntPtr readPipe=outputRead;reader=new Thread(()=>display.Read(readPipe));reader.IsBackground=true;reader.Start();
            display.Wait(()=>display.childPid>0,5000);
            int editorId=display.childPid;editor=OpenProcess(0x00100001,false,editorId);if(editor==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
            display.Wait(()=>display.Text().Contains("F3_VISIBLE_"),5000);
            int column=1;for(int i=0;i<3;i++){column++;int expected=column;display.Send("\x1b[C");display.Wait(()=>display.Text().Contains("Col "+expected+" "),3000);}
            int clearBefore;long bytesBefore;lock(display.sync){clearBefore=display.clears;bytesBefore=display.bytes;}
            List<double> timings=new List<double>();for(int i=0;i<31;i++){column++;int expected=column;long start=Stopwatch.GetTimestamp();display.Send("\x1b[C");display.Wait(()=>display.Text().Contains("Col "+expected+" "),3000);timings.Add((Stopwatch.GetTimestamp()-start)*1000.0/Stopwatch.Frequency);}
            timings.Sort();Result result=new Result();result.samples=timings.Count;result.injection_to_vt_p50_ms=timings[15];result.injection_to_vt_p95_ms=timings[29];
            lock(display.sync){result.clears_during_keys=display.clears-clearBefore;result.keyboard_output_bytes=display.bytes-bytesBefore;}
            if(result.clears_during_keys!=0)throw new IOException("Incremental keys unexpectedly cleared the display");
            using(Process process=Process.GetProcessById(editorId)){process.Refresh();TimeSpan before=process.TotalProcessorTime;Thread.Sleep(250);process.Refresh();result.idle_cpu_ms=(process.TotalProcessorTime-before).TotalMilliseconds;result.private_bytes=process.PrivateMemorySize64;}
            int resizeBefore;lock(display.sync){resizeBefore=display.clears;}
            display.Resize(120,30);HResult(ResizePseudoConsole(console,new Coord(120,30)));display.Wait(()=>display.clears>resizeBefore && display.Text().Contains("Group 1"),3000);
            lock(display.sync){result.resize_clears=display.clears-resizeBefore;}
            if(document){
                display.Send("\x06");display.Wait(()=>display.Text().Contains("literal"),3000);
                display.Send("STRASSE");display.Wait(()=>display.Text().Contains("Document search Complete: 1 matches"),3000);result.document_search_folded_without_more_input=true;
                display.Send("\x1bOR");display.Wait(()=>display.Text().Contains("Ln 2 Col 7"),3000);
                display.Send("\x1b");display.Wait(()=>!display.Text().Contains("literal"),3000);
                display.Send("\x07");display.Wait(()=>display.Text().Contains("Go to line[:column]"),3000);
                display.Send("2:2\r");display.Wait(()=>display.Text().Contains("Ln 2 Col 2"),3000);result.document_goto_scalars=true;
                display.Send("\x1b"+"s");display.Wait(()=>display.Text().Contains("Find in Project"),3000);
                display.Send("\x1b[B\r");display.Wait(()=>display.Text().Contains("Replace:"),3000);
                display.Send("\tSTRASSE\r");display.Wait(()=>display.Text().Contains("STRASSE NIÑO"),3000);
                if(!File.ReadAllText(path).Contains("Straße NIÑO"))throw new IOException("Document replacement unexpectedly saved the file");
                display.Send("\x1b");display.Wait(()=>!display.Text().Contains("Replace:"),3000);
                display.Send("\x1a");display.Wait(()=>display.Text().Contains("Straße NIÑO") && display.Text().Contains("Document search Complete"),3000);result.document_replaced_and_undone=true;
            }
            if(project){
                display.Send("\x1b"+"f");display.Wait(()=>display.Text().Contains("Exit"),3000);
                display.Send("\x1b[B\x1b[B\r");display.Wait(()=>display.Text().Contains("Open folder"),3000);
                display.Send(fixtureDir+"\r");display.Wait(()=>!display.Text().Contains("Open folder"),3000);
                display.Send("\x1b"+"s");display.Wait(()=>display.Text().Contains("Find in Project"),3000);
                display.Send("\x1b[B\x1b[B\x1b[B\x1b[B\r");display.Wait(()=>display.Text().Contains("Find in Project: literal"),3000);
                display.Send("F3_VISIBLE_\r");display.Wait(()=>display.Text().Contains("Include globs"),3000);
                display.Send("*.txt\r");display.Wait(()=>display.Text().Contains("Extra exclude globs"),3000);
                display.Send("\r");
                // No keyboard input until both asynchronous output and completion arrive.
                display.Wait(()=>display.Text().Contains("source.txt:1:1") && display.Text().Contains("Project search Complete"),5000);
                result.project_search_delivered=true;display.Send("\r");display.Wait(()=>display.Text().Contains("Col 1 "),3000);
                for(int pass=0;pass<2;pass++){
                    display.Send("\x1b"+"s");display.Wait(()=>display.Text().Contains("Replace in Project"),3000);
                    display.Send("\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r");display.Wait(()=>display.Text().Contains("Replacement text"),3000);
                    display.Send("F3_REPLACED_\r");display.Wait(()=>display.Text().Contains("+ F3_REPLACED_"),3000);result.project_replace_previewed=true;
                    if(pass==0){display.Send("\x1b");display.Wait(()=>!display.Text().Contains("+ F3_REPLACED_"),3000);}else{
                        display.Send("\x1b[20~");display.Wait(()=>display.Text().Contains("Replacement finished") && display.Text().Contains("F3_REPLACED_"),3000);
                        if(File.ReadAllText(path).Contains("F3_REPLACED_"))throw new IOException("Open buffer was unexpectedly saved by project replacement");
                        display.Send("\r\x1a");display.Wait(()=>display.Text().Contains("F3_VISIBLE_") && display.Text().Contains("Ready"),3000);result.project_replace_applied_and_undone=true;
                    }
                }
            }
            // File is clean: File menu -> Up wraps to Exit -> Enter, with no writes.
            display.Send("\x1b"+"f");
            display.Wait(()=>display.Text().Contains("Exit"),3000);
            display.Send("\x1b[A\r");
            if(WaitForSingleObject(child.process,5000)!=0)throw new TimeoutException("Editor did not exit through File/Exit");
            GetExitCodeProcess(child.process,out result.exit_code);if(result.exit_code!=0)throw new IOException("Editor exited with code "+result.exit_code);
            return result;
        }finally{
            if(editor!=IntPtr.Zero){if(WaitForSingleObject(editor,0)!=0){TerminateProcess(editor,99);WaitForSingleObject(editor,3000);}CloseHandle(editor);}
            if(child.process!=IntPtr.Zero){if(WaitForSingleObject(child.process,0)!=0){TerminateProcess(child.process,99);WaitForSingleObject(child.process,3000);}CloseHandle(child.process);CloseHandle(child.thread);}
            if(console!=IntPtr.Zero)ClosePseudoConsole(console);
            if(reader!=null)reader.Join(1000);
            foreach(IntPtr handle in new[]{inputRead,inputWrite,outputRead,outputWrite})if(handle!=IntPtr.Zero)CloseHandle(handle);
            if(attributes!=IntPtr.Zero){DeleteProcThreadAttributeList(attributes);Marshal.FreeHGlobal(attributes);}
            if(created)File.Delete(path);
            if(fixtureDir!=null)Directory.Delete(fixtureDir,false);
        }
    }
}

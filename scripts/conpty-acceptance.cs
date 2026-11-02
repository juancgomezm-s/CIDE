// Extends the existing private ConPTY host. Measures only processes it creates.
using System;
using System.IO;
using System.Text;
using System.Diagnostics;
using System.Threading;
using System.Runtime.InteropServices;
using System.Collections.Generic;

public static partial class JcConPtySmoke {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct ProcessEntry {
        public uint size,usage,pid;public UIntPtr heap;public uint module,threads,parent;
        public int priority;public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)] public string name;
    }
    [DllImport("kernel32",SetLastError=true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags,uint pid);
    [DllImport("kernel32",CharSet=CharSet.Unicode)] static extern bool Process32FirstW(IntPtr snapshot,ref ProcessEntry entry);
    [DllImport("kernel32",CharSet=CharSet.Unicode)] static extern bool Process32NextW(IntPtr snapshot,ref ProcessEntry entry);
    static HashSet<int> OwnedConsoleIds() {
        var ids=new HashSet<int>();IntPtr snap=CreateToolhelp32Snapshot(2,0);
        if(snap==new IntPtr(-1))throw new System.ComponentModel.Win32Exception();
        try {ProcessEntry e=new ProcessEntry();e.size=(uint)Marshal.SizeOf(typeof(ProcessEntry));
            if(Process32FirstW(snap,ref e))do {
                if(e.parent==Process.GetCurrentProcess().Id && string.Equals(e.name,"conhost.exe",StringComparison.OrdinalIgnoreCase))ids.Add((int)e.pid);
            }while(Process32NextW(snap,ref e));
        } finally {CloseHandle(snap);}return ids;
    }
    sealed class MemorySampler {
        internal volatile bool stop;internal volatile string phase="launch";
        internal readonly List<string> rows=new List<string>();
        readonly Display display;readonly int host,console;readonly Stopwatch clock;
        internal MemorySampler(Display d,int h,int c,Stopwatch timer){display=d;host=h;console=c;clock=timer;}
        string Counters(int pid) {
            if(pid==0)return "0,0,0";
            try{using(Process p=Process.GetProcessById(pid)){p.Refresh();return p.PrivateMemorySize64+","+p.WorkingSet64+","+p.PeakPagedMemorySize64;}}
            catch(ArgumentException){return "0,0,0";}catch(InvalidOperationException){return "0,0,0";}
        }
        internal void Run(){while(!stop){int editor;lock(display.sync){editor=display.childPid;}
            rows.Add(phase+","+clock.Elapsed.TotalMilliseconds.ToString("F4",System.Globalization.CultureInfo.InvariantCulture)+","+editor+","+Counters(editor)+","+host+","+Counters(host)+","+console+","+Counters(console)+","+Counters(Process.GetCurrentProcess().Id));Thread.Sleep(5);}}
    }
    public sealed class AcceptanceResult {
        public double startup_ready_ms;public List<double> key_ms;public List<string> memory;
    }
    public static AcceptanceResult RunAcceptance(string executable,string host,string directory,string shape,int iteration) {
        string root=Path.GetFullPath(directory);string fixture=Path.GetFullPath(Path.Combine(root,"work","m2-native-"+Process.GetCurrentProcess().Id+"-"+shape+"-"+iteration));
        if(!fixture.StartsWith(root.TrimEnd(Path.DirectorySeparatorChar)+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase)||Directory.Exists(fixture))throw new IOException("Unsafe/existing fixture");
        Directory.CreateDirectory(fixture);List<string> paths=new List<string>();
        IntPtr inputRead=IntPtr.Zero,inputWrite=IntPtr.Zero,outputRead=IntPtr.Zero,outputWrite=IntPtr.Zero,console=IntPtr.Zero,attributes=IntPtr.Zero,editor=IntPtr.Zero;
        ProcessInfo child=new ProcessInfo();Thread reader=null,sampling=null;MemorySampler meter=null;
        try {
            int documents=shape=="multi"?8:1;int size=shape=="long"?16*1024*1024:shape=="multi"?4*1024*1024:4096;
            for(int i=0;i<documents;i++) {
                string path=Path.Combine(fixture,"document-"+i+".txt");paths.Add(path);
                using(FileStream file=new FileStream(path,FileMode.CreateNew,FileAccess.Write)) {
                    byte[] first=Encoding.UTF8.GetBytes("M2_VISIBLE_"+i+" ");file.Write(first,0,first.Length);
                    byte[] block=Encoding.UTF8.GetBytes(shape=="long"?new string('x',8192):new string('x',63)+"\n");
                    while(file.Length<size){int count=(int)Math.Min(block.Length,size-file.Length);file.Write(block,0,count);}
                }
            }
            var consoles=OwnedConsoleIds();
            Check(CreatePipe(out inputRead,out inputWrite,IntPtr.Zero,0));Check(CreatePipe(out outputRead,out outputWrite,IntPtr.Zero,0));
            HResult(CreatePseudoConsole(new Coord(140,40),inputRead,outputWrite,0,out console));
            int consolePid=0;foreach(int id in OwnedConsoleIds())if(!consoles.Contains(id)){if(consolePid!=0)throw new IOException("Ambiguous owned console");consolePid=id;}
            CloseHandle(inputRead);inputRead=IntPtr.Zero;CloseHandle(outputWrite);outputWrite=IntPtr.Zero;
            IntPtr bytes=IntPtr.Zero;InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref bytes);attributes=Marshal.AllocHGlobal(bytes);Check(InitializeProcThreadAttributeList(attributes,1,0,ref bytes));
            Check(UpdateProcThreadAttribute(attributes,0,new IntPtr(0x00020016),console,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero));
            StartupEx startup=new StartupEx();startup.startup.cb=Marshal.SizeOf(typeof(StartupEx));startup.attributes=attributes;
            var command=new StringBuilder(Quote(host)+" "+Quote(executable)+" --encoding utf8");foreach(string path in paths)command.Append(" "+Quote(path.Substring(root.Length+1)));
            Stopwatch timer=Stopwatch.StartNew();
            Check(CreateProcessW(host,command,IntPtr.Zero,IntPtr.Zero,false,0x00080000,IntPtr.Zero,root,ref startup,out child));
            Display display=new Display(inputWrite);display.Resize(140,40);IntPtr pipe=outputRead;reader=new Thread(()=>display.Read(pipe));reader.IsBackground=true;reader.Start();
            meter=new MemorySampler(display,child.processId,consolePid,timer);sampling=new Thread(meter.Run);sampling.IsBackground=true;sampling.Start();
            display.Wait(()=>display.childPid>0,10000);editor=OpenProcess(0x00100001,false,display.childPid);if(editor==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();
            display.Wait(()=>display.Text().Contains("M2_VISIBLE_"+(documents-1)),10000);
            display.Send("\x1b[C");display.Wait(()=>display.Text().Contains("Col 2 "),10000);
            var result=new AcceptanceResult();result.startup_ready_ms=timer.Elapsed.TotalMilliseconds;
            meter.phase="ready";Thread.Sleep(50);
            display.Resize(140,40);HResult(ResizePseudoConsole(console,new Coord(140,40)));display.Wait(()=>display.Text().Contains("Group 1"),10000);
            // Ctrl+Shift+P encoded as CSI-u is not universally available; use the
            // menu's existing Split Right handler through its visible item.
            display.Send("\x1b"+"v");display.Wait(()=>display.Text().Contains("Split Right"),5000);
            // View items are stable commands; Home isn't a menu operation, so
            // walk from the initial Explorer item to Split Right.
            display.Send("\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r");
            display.Wait(()=>display.Text().Contains("Group 2"),5000);
            meter.phase="split";Thread.Sleep(50);
            result.key_ms=new List<double>();
            for(int i=0;i<36;i++) {int expected=i+3;long started=Stopwatch.GetTimestamp();display.Send("\x1b[C");display.Wait(()=>display.Text().Contains("Col "+expected+" "),5000);
                if(i>=5)result.key_ms.Add((Stopwatch.GetTimestamp()-started)*1000.0/Stopwatch.Frequency);}
            if(documents>1) {
                meter.phase="visit_documents";
                for(int i=0;i<documents;i++) {
                    // Alt+digit selects a catalog tab without opening a file.
                    display.Send("\x1b"+(i+1));int expected=i;
                    display.Wait(()=>display.Text().Contains("Group 1: document-"+expected+".txt"),10000);
                }
            }
            meter.phase="retained";Thread.Sleep(100);
            // Clean fixtures: close the second group, then every document via
            // centrally registered commands. Record retained process memory.
            display.Send("\x1b"+"v");display.Wait(()=>display.Text().Contains("One Group"),5000);
            display.Send("\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r");
            display.Wait(()=>!display.Text().Contains("Group 2"),5000);
            for(int i=0;i<documents;i++){display.Send("\x17");int remaining=documents-i-1;
                display.Wait(()=>remaining==0?display.Text().Contains("Group 1: Empty"):!display.Text().Contains("document-"+(remaining)+".txt"),5000);}
            meter.phase="closed";Thread.Sleep(100);
            display.Send("\x1b"+"f");display.Wait(()=>display.Text().Contains("Exit"),5000);display.Send("\x1b[A\r");
            if(WaitForSingleObject(child.process,10000)!=0)throw new TimeoutException("Acceptance exit");int code;GetExitCodeProcess(child.process,out code);if(code!=0)throw new IOException("Editor exit "+code);
            meter.stop=true;sampling.Join();result.memory=meter.rows;return result;
        } finally {
            if(meter!=null)meter.stop=true;if(sampling!=null)sampling.Join(2000);
            foreach(IntPtr process in new[]{editor,child.process})if(process!=IntPtr.Zero){if(WaitForSingleObject(process,0)!=0){TerminateProcess(process,99);WaitForSingleObject(process,3000);}CloseHandle(process);}
            if(child.thread!=IntPtr.Zero)CloseHandle(child.thread);if(console!=IntPtr.Zero)ClosePseudoConsole(console);if(reader!=null)reader.Join(1000);
            foreach(IntPtr handle in new[]{inputRead,inputWrite,outputRead,outputWrite})if(handle!=IntPtr.Zero)CloseHandle(handle);
            if(attributes!=IntPtr.Zero){DeleteProcThreadAttributeList(attributes);Marshal.FreeHGlobal(attributes);}
            foreach(string path in paths)File.Delete(path);Directory.Delete(fixture,false);
        }
    }
}

public static class MeasureM2Native {
    public static int Main(string[] args) {
        try {
            string root=Path.GetFullPath(args[0]);string phase=args[1];if(phase!="before"&&phase!="after")throw new ArgumentException("phase");
            string prefix=Path.Combine(root,"docs","benchmarks","M2-evidence-"+phase+"-native");
            if(File.Exists(prefix+"-timings.csv")||File.Exists(prefix+"-memory.csv"))throw new IOException("Refusing to overwrite retained measurements");
            using(var times=new StreamWriter(prefix+"-timings.csv",false,new UTF8Encoding(false)))
            using(var memory=new StreamWriter(prefix+"-memory.csv",false,new UTF8Encoding(false))) {
                times.NewLine="\n";memory.NewLine="\n";
                times.WriteLine("shape,run,operation,sample,elapsed_ms");
                memory.WriteLine("shape,run,phase,elapsed_ms,editor_pid,editor_private,editor_working,editor_peak_commit,host_pid,host_private,host_working,host_peak_commit,console_pid,console_private,console_working,console_peak_commit,harness_private,harness_working,harness_peak_commit");
                foreach(string shape in new[]{"base","multi","long"})for(int run=0;run<8;run++) {
                    var result=JcConPtySmoke.RunAcceptance(Path.Combine(root,"target/release/jc-ide-tui.exe"),Path.Combine(root,"target/release/examples/f3_console_host.exe"),root,shape,run);
                    if(run>0){times.WriteLine(shape+","+run+",startup_ready,0,"+result.startup_ready_ms.ToString("F4",System.Globalization.CultureInfo.InvariantCulture));
                        for(int i=0;i<result.key_ms.Count;i++)times.WriteLine(shape+","+run+",injection_to_vt,"+i+","+result.key_ms[i].ToString("F4",System.Globalization.CultureInfo.InvariantCulture));
                        foreach(string row in result.memory)memory.WriteLine(shape+","+run+","+row);}
                    times.Flush();memory.Flush();Console.WriteLine(shape+" "+run+" complete");
                }
            }return 0;
        }catch(Exception e){Console.Error.WriteLine(e);return 1;}
    }
}

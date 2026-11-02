// Opt-in measurement only: editor + validated descendants. Never launches/kills a process.
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Threading;

public sealed class GitAuxiliaryProbe : IDisposable {
    public const string Header="sample,phase,elapsed_ms,role,pid,start_ticks,parent_pid,name,private_bytes,working_set_bytes,peak_commit_bytes,cpu_total_ms";
    const int MaxSamples=4000,MaxRows=32000,MaxChars=8*1024*1024,MaxTracked=4096,MaxEntries=65536,MaxError=1024;
    const uint QueryAndSynchronize=0x00101000;
    static readonly CultureInfo Invariant=CultureInfo.InvariantCulture;
    readonly object sync=new object();
    readonly ManualResetEvent stop=new ManualResetEvent(false);
    readonly List<string> rows=new List<string>();
    readonly Dictionary<int,Tracked> tracked=new Dictionary<int,Tracked>();
    readonly int editorPid;
    Thread worker;
    bool started,stopped,disposed,truncated;
    string phase="before",error="";
    int chars=Header.Length+1;
    IReadOnlyList<string> frozen;

    [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct Entry {
        public uint size,usage,pid;public UIntPtr heap;public uint module,threads,parent;
        public int priority;public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)] public string name;
    }
    [StructLayout(LayoutKind.Sequential)] struct FileTime {
        public uint low,high;
        public ulong Value {get{return ((ulong)high<<32)|low;}}
    }
    [StructLayout(LayoutKind.Sequential)] struct Memory {
        public uint size,faults;
        public UIntPtr peakWorking,working,peakPagedPool,pagedPool,peakNonpagedPool,nonpagedPool,commit,peakCommit,privateBytes;
    }
    [DllImport("kernel32",SetLastError=true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags,uint pid);
    [DllImport("kernel32",EntryPoint="Process32FirstW",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)] static extern bool First(IntPtr snapshot,ref Entry entry);
    [DllImport("kernel32",EntryPoint="Process32NextW",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)] static extern bool Next(IntPtr snapshot,ref Entry entry);
    [DllImport("kernel32",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,int pid);
    [DllImport("kernel32",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FileTime created,out FileTime exited,out FileTime kernel,out FileTime user);
    [DllImport("kernel32",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr process,uint milliseconds);
    [DllImport("kernel32")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("psapi",SetLastError=true)] static extern bool GetProcessMemoryInfo(IntPtr process,ref Memory memory,uint size);
    sealed class Tracked {
        internal int pid,parent;
        internal string name;
        internal long start;
        internal IntPtr handle;
    }
    struct Metrics {
        internal ulong privateBytes,working,peakCommit;
        internal double cpu;
    }
    public GitAuxiliaryProbe(int editorPid) {
        this.editorPid=editorPid;
        if(editorPid<=0){SetError("Invalid editor PID");return;}
        try {
            Tracked editor=Open(editorPid,0,"");
            if(editor==null){SetError("Editor was not alive when binding the probe");return;}
            tracked.Add(editorPid,editor);
        } catch(Exception e){SetError("Editor identity: "+e.Message);}
    }
    public string Phase {set{lock(sync){phase=Bound(value??"",128);}}}
    public bool Truncated {get{lock(sync){return truncated;}}}
    public string Error {get{lock(sync){return error;}}}
    public IReadOnlyList<string> Rows {
        get {lock(sync){
            if(frozen!=null)return frozen;
            IReadOnlyList<string> copy=new ReadOnlyCollection<string>(rows.ToArray());
            if(stopped&&worker==null)frozen=copy;
            return copy;
        }}
    }
    public void Start() {
        lock(sync){
            if(disposed)throw new ObjectDisposedException("GitAuxiliaryProbe");
            if(started)return;
            if(stopped)throw new InvalidOperationException("Probe is one-shot and already stopped");
            started=true;
            if(error.Length!=0){stopped=true;ReleaseHandles();return;}
            worker=new Thread(Run);worker.Name="JC Git auxiliary sample";worker.IsBackground=true;
            try{worker.Start();}catch(Exception e){worker=null;stopped=true;SetError("Sampler start: "+e.Message);ReleaseHandles();}
        }
    }
    public void Stop() {
        Thread thread;
        lock(sync){if(disposed)return;stopped=true;stop.Set();thread=worker;}
        if(thread!=null&&thread!=Thread.CurrentThread)thread.Join();
        lock(sync){if(worker==thread)worker=null;ReleaseHandles();}
    }
    public void Dispose() {
        Stop();
        lock(sync){if(disposed)return;disposed=true;stop.Dispose();}
    }
    static string Bound(string value,int limit) {
        if(value.Length<=limit)return value;
        int end=limit;if(end>0&&char.IsHighSurrogate(value[end-1]))end--;
        return value.Substring(0,end);
    }
    void SetError(string value) {
        value=Bound(value.Replace('\r',' ').Replace('\n',' '),MaxError);
        lock(sync){if(error.Length==0)error=value;}
    }
    void Truncate(){lock(sync){truncated=true;}stop.Set();}
    static string Csv(string value){return "\""+(value??"").Replace("\"","\"\"")+"\"";}
    static long StartTicks(FileTime value){return DateTime.FromFileTimeUtc(checked((long)value.Value)).Ticks;}
    static bool Times(IntPtr handle,out long created,out long exited,out double cpu) {
        FileTime start,end,kernel,user;created=0;exited=0;cpu=0;
        if(!GetProcessTimes(handle,out start,out end,out kernel,out user))return false;
        created=StartTicks(start);
        // Exit FILETIME is undefined while a process is alive.
        if(WaitForSingleObject(handle,0)==0&&end.Value!=0)exited=StartTicks(end);
        cpu=kernel.Value/10000.0+user.Value/10000.0;return true;
    }
    static Tracked Open(int pid,int parent,string name) {
        IntPtr handle=OpenProcess(QueryAndSynchronize,false,pid);
        if(handle==IntPtr.Zero){
            int code=Marshal.GetLastWin32Error();
            if(code==87||code==1168)return null; // exited between snapshot and OpenProcess
            throw new Win32Exception(code);
        }
        try {
            long created,exited;double cpu;
            if(!Times(handle,out created,out exited,out cpu))throw new Win32Exception(Marshal.GetLastWin32Error());
            if(created<=0)throw new InvalidOperationException("Missing process creation time");
            return new Tracked{pid=pid,parent=parent,name=name??"",start=created,handle=handle};
        }catch{CloseHandle(handle);throw;}
    }
    Dictionary<int,Entry> Snapshot() {
        IntPtr snapshot=CreateToolhelp32Snapshot(2,0);
        if(snapshot==new IntPtr(-1))throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            var entries=new Dictionary<int,Entry>();Entry entry=new Entry();entry.size=(uint)Marshal.SizeOf(typeof(Entry));
            if(!First(snapshot,ref entry)){
                int code=Marshal.GetLastWin32Error();if(code==18)return entries;throw new Win32Exception(code);
            }
            do {
                if(entries.Count>=MaxEntries){Truncate();return null;}
                entries[(int)entry.pid]=entry;
            }while(Next(snapshot,ref entry));
            int last=Marshal.GetLastWin32Error();if(last!=18)throw new Win32Exception(last);
            return entries;
        }finally{CloseHandle(snapshot);}
    }
    bool ParentWindow(Tracked parent,long childStart) {
        long created,exited;double cpu;
        if(!Times(parent.handle,out created,out exited,out cpu))throw new Win32Exception(Marshal.GetLastWin32Error());
        if(created!=parent.start)throw new InvalidOperationException("Pinned parent identity changed");
        if(childStart<created)return false;
        uint state=WaitForSingleObject(parent.handle,0);
        if(state==0)return exited>0&&childStart<=exited;
        if(state==258)return true;
        throw new Win32Exception(Marshal.GetLastWin32Error());
    }
    bool Admit(Dictionary<int,Entry> entries) {
        bool added;
        do {
            added=false;
            foreach(var pair in entries){
                if(stop.WaitOne(0))return false;
                int pid=pair.Key;Entry entry=pair.Value;
                if(pid==editorPid||tracked.ContainsKey(pid))continue;
                Tracked parent;if(!tracked.TryGetValue((int)entry.parent,out parent))continue;
                if(tracked.Count>=MaxTracked){Truncate();return false;}
                Tracked candidate=Open(pid,(int)entry.parent,entry.name);
                if(candidate==null)continue;
                bool retained=false;
                try {
                    // The PID may have died/reused after the first Toolhelp
                    // snapshot but before OpenProcess. Confirm PPID/name after
                    // pinning the new handle, before admitting any counters.
                    var confirmation=Snapshot();if(confirmation==null)return false;
                    Entry confirmed;
                    if(!confirmation.TryGetValue(pid,out confirmed)||confirmed.parent!=entry.parent
                        || !string.Equals(confirmed.name,entry.name,StringComparison.OrdinalIgnoreCase))continue;
                    if(!ParentWindow(parent,candidate.start))continue;
                    tracked.Add(pid,candidate);retained=true;added=true;
                }finally{if(!retained)CloseHandle(candidate.handle);}
            }
        }while(added);
        return true;
    }
    bool Read(Tracked process,out Metrics metrics) {
        metrics=new Metrics();
        uint state=WaitForSingleObject(process.handle,0);
        if(state==0)return false;
        if(state!=258)throw new Win32Exception(Marshal.GetLastWin32Error());
        long created,exited;double cpu;
        if(!Times(process.handle,out created,out exited,out cpu)){
            if(WaitForSingleObject(process.handle,0)==0)return false;
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        if(created!=process.start)throw new InvalidOperationException("Pinned PID/start identity changed");
        Memory memory=new Memory();memory.size=(uint)Marshal.SizeOf(typeof(Memory));
        if(!GetProcessMemoryInfo(process.handle,ref memory,memory.size)){
            if(WaitForSingleObject(process.handle,0)==0)return false;
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        metrics.privateBytes=memory.privateBytes.ToUInt64();metrics.working=memory.working.ToUInt64();
        metrics.peakCommit=memory.peakCommit.ToUInt64();metrics.cpu=cpu;return true;
    }
    string Row(int sample,string stage,double elapsed,Tracked process,Metrics metrics) {
        return sample.ToString(Invariant)+","+Csv(stage)+","+elapsed.ToString("F4",Invariant)+","+
            (process.pid==editorPid?"editor":"auxiliary")+","+process.pid.ToString(Invariant)+","+
            process.start.ToString(Invariant)+","+process.parent.ToString(Invariant)+","+Csv(process.name)+","+
            metrics.privateBytes.ToString(Invariant)+","+metrics.working.ToString(Invariant)+","+
            metrics.peakCommit.ToString(Invariant)+","+metrics.cpu.ToString("F4",Invariant);
    }
    bool Sample(int sample,string stage,double elapsed) {
            var entries=Snapshot();if(entries==null)return false;
        Tracked editor=tracked[editorPid];Entry root;
        if(!entries.TryGetValue(editorPid,out root)){
            if(WaitForSingleObject(editor.handle,0)==0)return false;
            throw new InvalidOperationException("Live editor missing from Toolhelp snapshot");
        }
        editor.parent=(int)root.parent;editor.name=root.name??"";
        if(!Admit(entries))return false;
        var current=new List<string>();Metrics metrics;
        if(!Read(editor,out metrics))return false;
        current.Add(Row(sample,stage,elapsed,editor,metrics));
        foreach(Tracked process in tracked.Values){
            if(process.pid==editorPid||!entries.ContainsKey(process.pid))continue;
            if(Read(process,out metrics))current.Add(Row(sample,stage,elapsed,process,metrics));
        }
        int extra=0;foreach(string row in current)extra=checked(extra+row.Length+1);
        lock(sync){
            // Whole samples only: no quota-truncated concurrent sum is published.
            if(current.Count>MaxRows-rows.Count||extra>MaxChars-chars){truncated=true;return false;}
            rows.AddRange(current);chars+=extra;
        }
        return true;
    }
    void Run() {
        Stopwatch clock=Stopwatch.StartNew();
        try {
            int sample=0;
            while(!stop.WaitOne(0)){
                if(sample>=MaxSamples){Truncate();break;}
                string stage;lock(sync){stage=phase;}
                double elapsed=clock.Elapsed.TotalMilliseconds;
                if(!Sample(sample,stage,elapsed))break;
                sample++;
                // Requested cadence; collection time and scheduler add delay.
                if(stop.WaitOne(5))break;
            }
        }catch(Exception e){SetError("Sampler: "+e.Message);}
        finally{ReleaseHandles();}
    }
    void ReleaseHandles() {
        foreach(Tracked process in tracked.Values){if(process.handle!=IntPtr.Zero){CloseHandle(process.handle);process.handle=IntPtr.Zero;}}
    }
}

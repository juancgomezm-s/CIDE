// Opt-in local process evidence. Compile with conpty-smoke/git/git-metrics.cs.
using System;
using System.IO;
using System.Text;
using System.Diagnostics;
using System.Globalization;

public static class MeasureGitAuxiliary {
    static string Json(string value){
        var output=new StringBuilder("\"");
        foreach(char ch in value){
            if(ch=='"'||ch=='\\')output.Append('\\').Append(ch);
            else if(ch<32)output.Append("\\u").Append(((int)ch).ToString("x4"));
            else output.Append(ch);
        }
        return output.Append('"').ToString();
    }
    public static int Main(string[] args){
        try {
            if(args.Length!=3||!Path.IsPathRooted(args[1]))throw new ArgumentException("Usage: measure-git-auxiliary ROOT ABSOLUTE_GIT_EXE OUTPUT_PREFIX");
            string root=Path.GetFullPath(args[0]),git=Path.GetFullPath(args[1]);
            string prefix=Path.GetFullPath(args[2]);
            if(!prefix.StartsWith(root.TrimEnd(Path.DirectorySeparatorChar)+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase))throw new IOException("Output outside workspace");
            foreach(string suffix in new[]{"-processes.csv","-runs.csv","-metadata.json"})if(File.Exists(prefix+suffix))throw new IOException("Refusing to overwrite retained measurements");
            string executable=Path.Combine(root,"target/release/jc-ide-tui.exe"),host=Path.Combine(root,"target/release/examples/f3_console_host.exe");
            using(var samples=new StreamWriter(prefix+"-processes.csv",false,new UTF8Encoding(false)))
            using(var runs=new StreamWriter(prefix+"-runs.csv",false,new UTF8Encoding(false))){
                samples.NewLine="\n";runs.NewLine="\n";
                samples.WriteLine("fixture_bytes,run,"+GitAuxiliaryProbe.Header);
                runs.WriteLine("fixture_bytes,run,elapsed_ms,sample_rows,head_async,index_async,hunk_navigation,stale_after_undo,sidebar_survives_hidden_bottom,disk_index_unchanged,exit_code,truncated,sampling_failed");
                foreach(int size in new[]{4096,1024*1024,2*1024*1024-64})for(int run=0;run<3;run++){
                    var clock=Stopwatch.StartNew();
                    var result=JcConPtySmoke.Run(executable,host,root,false,false,git,true,size);
                    foreach(string row in JcConPtySmoke.LastGitSamples)samples.WriteLine(size+","+run+","+row);
                    runs.WriteLine(string.Join(",",new[]{size.ToString(),run.ToString(),clock.Elapsed.TotalMilliseconds.ToString("F4",CultureInfo.InvariantCulture),result.git_metrics_rows.ToString(),result.git_head_delivered_without_more_input.ToString(),result.git_index_delivered_without_more_input.ToString(),result.git_hunk_navigation.ToString(),result.git_stale_after_undo.ToString(),result.git_sidebar_hunks_survive_hidden_bottom.ToString(),result.git_disk_and_index_unchanged.ToString(),result.exit_code.ToString(),result.git_metrics_truncated.ToString(),result.git_metrics_failed.ToString()}));
                    samples.Flush();runs.Flush();
                    if(result.git_metrics_failed||result.git_metrics_truncated)throw new IOException("Incomplete process samples: "+JcConPtySmoke.LastGitSamplingError);
                    Console.WriteLine(size+" bytes, run "+run+": "+result.git_metrics_rows+" process rows");
                }
            }
            string metadata="{\n  \"utc\": "+Json(DateTime.UtcNow.ToString("o"))+",\n  \"os_reported\": "+Json(Environment.OSVersion.VersionString)+",\n  \"framework\": "+Json(Environment.Version.ToString())+",\n  \"git_executable\": "+Json(git)+",\n  \"git_file_version\": "+Json(FileVersionInfo.GetVersionInfo(git).FileVersion??"")+",\n  \"editor\": "+Json(executable)+",\n  \"requested_sample_ms\": 5,\n  \"runs_per_size\": 3,\n  \"scope\": \"editor and observed descendants during Git flow, including Git conhosts; excludes initial TUI host/conhost/harness\",\n  \"limits\": \"sampling can miss short-lived processes and true peaks; no physical display or global acceptance\"\n}\n";
            File.WriteAllText(prefix+"-metadata.json",metadata,new UTF8Encoding(false));return 0;
        }catch(Exception error){Console.Error.WriteLine(error);return 1;}
    }
}

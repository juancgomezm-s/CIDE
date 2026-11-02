// Companion for conpty-smoke.cs; Git flow is opt-in. Writes are synthetic fixture setup.
using System;
using System.IO;
using System.Text;
using System.Diagnostics;

public static partial class JcConPtySmoke {
    static string FixtureGit(string executable,string root,params string[] args){
        var start=new ProcessStartInfo(executable);
        start.UseShellExecute=false;start.CreateNoWindow=true;start.WorkingDirectory=root;
        start.RedirectStandardInput=true;start.RedirectStandardOutput=true;start.RedirectStandardError=true;
        start.EnvironmentVariables.Clear();
        foreach(string key in new[]{"SYSTEMROOT","WINDIR","TEMP","TMP"}){string value=Environment.GetEnvironmentVariable(key);if(value!=null)start.EnvironmentVariables[key]=value;}
        foreach(string key in new[]{"GIT_CONFIG_SYSTEM","GIT_CONFIG_GLOBAL"})start.EnvironmentVariables[key]="NUL";
        start.EnvironmentVariables["GIT_CONFIG_NOSYSTEM"]="1";start.EnvironmentVariables["GIT_CONFIG_COUNT"]="0";
        foreach(string key in new[]{"GIT_AUTHOR_NAME","GIT_COMMITTER_NAME"})start.EnvironmentVariables[key]="JC fixture";
        foreach(string key in new[]{"GIT_AUTHOR_EMAIL","GIT_COMMITTER_EMAIL"})start.EnvironmentVariables[key]="fixture@example.invalid";
        var command=new StringBuilder("--no-pager -c core.hooksPath=NUL -c core.fsmonitor=false -c core.autocrlf=false");
        foreach(string arg in args)command.Append(" ").Append(Quote(arg));start.Arguments=command.ToString();
        using(var child=Process.Start(start)){
            child.StandardInput.Close();
            if(!child.WaitForExit(5000)){child.Kill();child.WaitForExit();throw new TimeoutException("Synthetic Git fixture setup");}
            string output=child.StandardOutput.ReadToEnd();string error=child.StandardError.ReadToEnd();
            if(child.ExitCode!=0)throw new IOException("Synthetic Git fixture: "+error);
            return output.Trim();
        }
    }
    static void SeedGitFixture(string git,string root,string path){
        FixtureGit(git,root,"init","--quiet","--template=","--initial-branch=main");
        string head=FixtureGit(git,root,"hash-object","-w","--no-filters","--",Path.GetFileName(path));
        FixtureGit(git,root,"update-index","--add","--cacheinfo","100644,"+head+",source.txt");
        string tree=FixtureGit(git,root,"write-tree");
        string commit=FixtureGit(git,root,"commit-tree",tree,"-m","synthetic native diff");
        FixtureGit(git,root,"update-ref","HEAD",commit);
        string staged=Path.Combine(root,"staged-object");File.WriteAllText(staged,"INDEX_ONLY\r\nStraße NIÑO\r\n",new UTF8Encoding(false));
        string index=FixtureGit(git,root,"hash-object","-w","--no-filters","--","staged-object");
        FixtureGit(git,root,"update-index","--cacheinfo","100644,"+index+",source.txt");
    }
    static void CheckGitFlow(Display display,Result result,string git,string root,string path){
        byte[] original=File.ReadAllBytes(path),index=File.ReadAllBytes(Path.Combine(root,".git/index"));
        // A single input character is exactly one edit on Windows ConPTY too.
        display.Send("Z");display.Wait(()=>display.Text().Contains("xZx"),3000);
        display.Send("\x1b"+"f");display.Wait(()=>display.Text().Contains("Exit"),3000);
        display.Send("\x1b[B\x1b[B\r");display.Wait(()=>display.Text().Contains("Open folder"),3000);
        display.Send(root+"\r");display.Wait(()=>!display.Text().Contains("Open folder"),3000);
        display.Send("\x1b"+"v");display.Wait(()=>display.Text().Contains("Active File Diff"),3000);
        display.Send("\x1b[B\x1b[B\r");display.Wait(()=>display.Text().Contains("Absolute path to Git"),3000);
        display.Send(git+"\r");
        // Deliberately no further key while the JobSystem completion wakes the UI.
        display.Wait(()=>display.Text().Contains("Git snapshot comparison ready")&&display.Text().Contains("HEAD snapshot")&&display.Text().Contains("[CRLF]"),10000);
        result.git_head_delivered_without_more_input=true;
        ToggleGitBottom(display);display.Wait(()=>display.Text().Contains("> 1 @@")&&!display.Text().Contains("[CRLF]"),3000);
        result.git_sidebar_hunks_survive_hidden_bottom=true;
        ToggleGitBottom(display);display.Wait(()=>display.Text().Contains("[CRLF]"),3000);
        display.Send("\r");display.Wait(()=>display.Text().Contains("Git hunk 1 selected")&&display.Text().Contains("Col 1 "),3000);result.git_hunk_navigation=true;
        display.Send("\x1a");display.Wait(()=>display.Text().Contains("Stale:")&&!display.Text().Contains("xZx"),3000);result.git_stale_after_undo=true;
        display.Send("\x1b"+"v");display.Wait(()=>display.Text().Contains("Compare Index Snapshot"),3000);
        display.Send("\x1b[A\x1b[A\x1b[A\x1b[A\r");
        display.Wait(()=>display.Text().Contains("Git snapshot comparison ready")&&display.Text().Contains("index snapshot")&&display.Text().Contains("INDEX_ONLY"),10000);
        result.git_index_delivered_without_more_input=true;
        if(Convert.ToBase64String(File.ReadAllBytes(path))!=Convert.ToBase64String(original)||Convert.ToBase64String(File.ReadAllBytes(Path.Combine(root,".git/index")))!=Convert.ToBase64String(index))throw new IOException("Read-only Git flow changed file or index");
        result.git_disk_and_index_unchanged=true;
    }
    static void ToggleGitBottom(Display display){
        display.Send("\x1b"+"v");display.Wait(()=>display.Text().Contains("Toggle Bottom Panel"),3000);
        display.Send("\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r");
    }
}

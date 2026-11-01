use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jc-ide-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jc-ide"));
        command.current_dir(&self.0);
        command
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn successful(output: Output) {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn actual_cli_roundtrip_preserves_all_byte_values() {
    let work = Workspace::new();
    let bytes: Vec<u8> = (0..=255)
        .chain([b'\r', b'\n', b'\n', b'\r', 0xf1])
        .collect();
    fs::write(work.0.join("legacy.p"), &bytes).unwrap();
    successful(
        work.command()
            .args(["roundtrip", "legacy.p", "copy.p"])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read(work.0.join("copy.p")).unwrap(), bytes);
    assert_eq!(fs::read(work.0.join("legacy.p")).unwrap(), bytes);
}

#[test]
fn actual_cli_edits_a_copy_and_refuses_existing_output() {
    let work = Workspace::new();
    let bytes = b"X\xf1Y\r\n";
    fs::write(work.0.join("source.p"), bytes).unwrap();
    successful(
        work.command()
            .args(["edit", "source.p", "1", "2", "FF00", "new.p"])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read(work.0.join("new.p")).unwrap(), b"X\xff\0Y\r\n");
    assert_eq!(fs::read(work.0.join("source.p")).unwrap(), bytes);
    let refused = work
        .command()
        .args(["edit", "source.p", "1", "2", "41", "new.p"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(fs::read(work.0.join("new.p")).unwrap(), b"X\xff\0Y\r\n");
}

#[test]
fn invalid_range_does_not_create_output() {
    let work = Workspace::new();
    fs::write(work.0.join("source.p"), b"small").unwrap();
    let output = work
        .command()
        .args(["edit", "source.p", "0", "99", "FF", "bad.p"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!work.0.join("bad.p").exists());
    assert_eq!(fs::read(work.0.join("source.p")).unwrap(), b"small");
}

#[test]
fn progress_result_exit_status_reflects_the_complete_response() {
    let work = Workspace::new();
    let header = "JCIDE|1|49534F383835392D31|49424D383530\n";
    for (suffix, expected_success) in [
        ("END|OK\n", true),
        ("END|ERROR\n", false),
        ("DIAG|error|-|-|-|426164\nEND|OK\n", false),
        ("", false),
    ] {
        fs::write(work.0.join("response.txt"), format!("{header}{suffix}")).unwrap();
        let output = work
            .command()
            .args(["progress-result", "response.txt"])
            .output()
            .unwrap();
        assert_eq!(output.status.success(), expected_success, "{suffix}");
    }
}

#[test]
fn plan_only_prepares_files_without_launching_an_executable() {
    let work = Workspace::new();
    let job = work.0.join("new-job");
    let nonexistent_exe = work.0.join("not-an-installed-avm.exe");
    let helper = work.0.join("helper.p");
    let source = work.0.join("source.p");
    let mut command = work.command();
    command
        .args(["progress-plan", "--action", "check", "--exe"])
        .arg(&nonexistent_exe)
        .arg("--helper")
        .arg(&helper)
        .arg("--source")
        .arg(&source)
        .arg("--output-dir")
        .arg(&job)
        .args(["--cpstream", "iso8859-1", "--cpinternal", "iso8859-1"]);
    successful(command.output().unwrap());
    let request = fs::read(job.join("request.txt")).unwrap();
    assert!(request.starts_with(b"JCIDE/1\ncheck\n"));
    assert!(!job.join("response.txt").exists());
    assert!(!command.output().unwrap().status.success());
    assert_eq!(fs::read(job.join("request.txt")).unwrap(), request);
}

#[cfg(not(windows))]
#[test]
fn windows_output_path_is_not_created_as_a_relative_directory_on_unix() {
    let work = Workspace::new();
    let output = work
        .command()
        .args([
            "progress-plan",
            "--action",
            "check",
            "--exe",
            r"C:\Progress\prowin32.exe",
            "--helper",
            r"C:\JC-IDE\helper.p",
            "--source",
            r"C:\sources\sample.p",
            "--output-dir",
            r"C:\Temp\job",
            "--cpstream",
            "iso8859-1",
            "--cpinternal",
            "iso8859-1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read_dir(&work.0).unwrap().count(), 0);
}

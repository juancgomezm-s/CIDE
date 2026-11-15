use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[test]
fn git_head_command_distinguishes_unborn_and_absent_with_no_git_in_path() {
    let fixture = std::env::temp_dir().join(format!("cide-cli-head-{}", std::process::id()));
    fs::create_dir(&fixture).unwrap();
    let fixture = fixture.canonicalize().unwrap();
    let cleanup = Fixture(&fixture);
    let probe = |path: &Path| {
        let output = Command::new(env!("CARGO_BIN_EXE_cide"))
            .arg("git-head")
            .arg(path)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["protocol"], "cide.git.head.v1");
        value
    };
    let absent = probe(&fixture);
    assert_eq!(absent["state"], "error");
    assert_eq!(absent["code"], "not_repository");

    fs::create_dir_all(fixture.join(".git/objects")).unwrap();
    fs::create_dir_all(fixture.join(".git/refs/heads")).unwrap();
    fs::write(fixture.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
    assert_eq!(probe(&fixture)["state"], "unborn");
    drop(cleanup);
}

struct Fixture<'a>(&'a Path);

impl Drop for Fixture<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0);
    }
}

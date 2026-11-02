//! Windows ConPTY test bootstrap: bind redirected parent stdio to this console.
//! Opt-in harness only; never used by the editor's normal launch path.
#[cfg(windows)]
fn main() {
    use std::{fs::OpenOptions, io::Write, process::Command};
    let mut args = std::env::args_os().skip(1);
    let executable = args.next().expect("editor executable");
    let input = OpenOptions::new().read(true).open("CONIN$").unwrap();
    let mut output = OpenOptions::new().write(true).open("CONOUT$").unwrap();
    let error = output.try_clone().unwrap();
    let mut child = Command::new(executable)
        .args(args)
        .stdin(input)
        .stdout(output.try_clone().unwrap())
        .stderr(error)
        .spawn()
        .unwrap();
    output
        .write_all(format!("JC_CHILD_PID={};\r\n", child.id()).as_bytes())
        .unwrap();
    std::process::exit(child.wait().unwrap().code().unwrap_or(1));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only ConPTY harness");
}

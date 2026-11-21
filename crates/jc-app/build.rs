fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    println!(
        "cargo:rerun-if-changed={}",
        root.join(".git/HEAD").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join(".git/refs/heads/main").display()
    );
    println!("cargo:rerun-if-env-changed=CIDE_RELEASE_VERSION");
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(&root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "sin-revision".into());
    let changed = std::process::Command::new("git")
        .args(["diff", "--quiet", "HEAD", "--", "crates", "build_support"])
        .current_dir(&root)
        .status()
        .is_ok_and(|status| status.code() == Some(1));
    println!(
        "cargo:rerun-if-changed={}",
        root.join(".git/index").display()
    );
    println!(
        "cargo:rustc-env=CIDE_REVISION={}{}",
        revision.trim(),
        if changed { " + cambios locales" } else { "" }
    );
    let version = std::env::var("CIDE_RELEASE_VERSION").unwrap_or_else(|_| "0.1.11-dev".into());
    println!("cargo:rustc-env=CIDE_RELEASE_VERSION={version}");
}

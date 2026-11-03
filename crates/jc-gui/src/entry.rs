//! Native frontend CLI; smoke mode operates only on caller-owned fixture paths.
use jc_gui::{GuiOptions, SmokeOptions};
use jc_text::TextEncoding;
use std::ffi::OsString;
use std::path::PathBuf;

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<GuiOptions>, String> {
    let mut options = GuiOptions::default();
    let mut fixture = None;
    let mut report = None;
    let mut bitmap = None;
    let mut navigation_only = false;
    let mut line_edit_only = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--smoke-navigation-only") => {
                if navigation_only {
                    return Err("duplicate --smoke-navigation-only".into());
                }
                navigation_only = true;
            }
            Some("--smoke-line-edit-only") => {
                if line_edit_only {
                    return Err("duplicate --smoke-line-edit-only".into());
                }
                line_edit_only = true;
            }
            Some("--git-executable") => {
                if options.git_executable.is_some() {
                    return Err("duplicate --git-executable".into());
                }
                let path = PathBuf::from(args.next().ok_or("missing Git executable path")?);
                if !path.is_absolute() || path.as_os_str().len() > 4096 {
                    return Err(
                        "--git-executable requires an absolute path within4096 native units".into(),
                    );
                }
                options.git_executable = Some(path);
            }
            Some("--search-workers") => {
                if options.search_workers.is_some() {
                    return Err("duplicate --search-workers".into());
                }
                let value = args
                    .next()
                    .and_then(|v| v.into_string().ok())
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| (1..=4).contains(n))
                    .ok_or("--search-workers requires1..4")?;
                options.search_workers = Some(value);
            }
            Some("--project-root") => {
                if options.project_root.is_some() {
                    return Err("duplicate --project-root".into());
                }
                options.project_root =
                    Some(PathBuf::from(args.next().ok_or("missing project root")?));
            }
            Some("--encoding") => {
                if options.encoding.is_some() {
                    return Err("duplicate --encoding".into());
                }
                options.encoding = Some(
                    match args.next().and_then(|s| s.into_string().ok()).as_deref() {
                        Some("utf-8" | "utf8") => TextEncoding::Utf8,
                        Some("cp1252" | "windows-1252") => TextEncoding::Windows1252,
                        Some("latin1" | "iso-8859-1") => TextEncoding::Iso8859_1,
                        Some("ibm850" | "cp850") => TextEncoding::Ibm850,
                        _ => return Err("use --encoding utf-8|cp1252|latin1|ibm850".into()),
                    },
                );
            }
            Some("--smoke-fixture" | "--smoke-report" | "--smoke-bitmap") => {
                let path = PathBuf::from(args.next().ok_or("missing smoke path")?);
                if !path.is_absolute() {
                    return Err("smoke paths must be absolute".into());
                }
                let target = match arg.to_str().expect("matched option") {
                    "--smoke-fixture" => &mut fixture,
                    "--smoke-report" => &mut report,
                    _ => &mut bitmap,
                };
                if target.replace(path).is_some() {
                    return Err("duplicate smoke path option".into());
                }
            }
            Some(word) if word.starts_with('-') => return Err(format!("unknown option: {word}")),
            _ => options.paths.push(PathBuf::from(arg)),
        }
    }
    if navigation_only && line_edit_only {
        return Err("choose only one smoke section".into());
    }
    if fixture.is_some()
        || report.is_some()
        || bitmap.is_some()
        || navigation_only
        || line_edit_only
    {
        if !options.paths.is_empty() || options.project_root.is_some() {
            return Err("smoke uses only its explicit fixture".into());
        }
        let fixture = fixture.ok_or("--smoke-fixture requires --smoke-report")?;
        let report = report.ok_or("--smoke-report requires --smoke-fixture")?;
        if fixture == report
            || bitmap
                .as_ref()
                .is_some_and(|p| p == &fixture || p == &report)
        {
            return Err("smoke fixture, report and bitmap must be distinct".into());
        }
        options.smoke = Some(SmokeOptions {
            fixture,
            report,
            bitmap,
            navigation_only,
            line_edit_only,
        });
    }
    Ok(Some(options))
}

pub(super) fn run() {
    match parse(std::env::args_os().skip(1)) {
        Ok(None) => println!("CIDE native GUI\nUsage: cide-gui [--project-root DIR] [--git-executable ABS] [--search-workers 1..4] [--encoding utf-8|cp1252|latin1|ibm850] [files...]\nLegacy alias: jc-ide-gui. Windows native frontend. Smoke: --smoke-fixture ABS --smoke-report ABS [--smoke-bitmap ABS]"),
        Ok(Some(options)) => {
            if let Err(error) = jc_gui::run(options) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}; use --help");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn git_executable_is_explicit_absolute_metadata_and_duplicate_or_missing_reject() {
        let path = std::env::current_dir()
            .unwrap()
            .join("trusted Git that need not exist.exe");
        let options = super::parse([
            std::ffi::OsString::from("--git-executable"),
            path.as_os_str().into(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(options.git_executable.as_ref(), Some(&path));
        for args in [
            vec!["--git-executable"],
            vec!["--git-executable", "relative.exe"],
        ] {
            assert!(super::parse(args.into_iter().map(std::ffi::OsString::from)).is_err());
        }
        assert!(super::parse([
            std::ffi::OsString::from("--git-executable"),
            path.as_os_str().into(),
            std::ffi::OsString::from("--git-executable"),
            path.as_os_str().into()
        ])
        .is_err());
    }
    #[test]
    fn project_workers_are_bounded_and_explicit() {
        let options = super::parse(
            ["--search-workers", "1"]
                .into_iter()
                .map(std::ffi::OsString::from),
        )
        .unwrap()
        .unwrap();
        assert_eq!(options.search_workers, Some(1));
        for value in ["0", "5", "bad"] {
            assert!(super::parse(
                ["--search-workers", value]
                    .into_iter()
                    .map(std::ffi::OsString::from)
            )
            .is_err());
        }
        assert!(super::parse(
            ["--search-workers"]
                .into_iter()
                .map(std::ffi::OsString::from)
        )
        .is_err());
        assert!(super::parse(
            ["--search-workers", "1", "--search-workers", "2"]
                .into_iter()
                .map(std::ffi::OsString::from)
        )
        .is_err());
    }
    use super::*;
    fn args(items: &[&str]) -> Result<Option<GuiOptions>, String> {
        parse(items.iter().map(OsString::from))
    }
    #[test]
    fn files_and_encodings_are_explicit_without_starting_a_window() {
        let options = args(&["--encoding", "cp1252", "legacy.p", "reference.i"])
            .unwrap()
            .unwrap();
        assert_eq!(options.encoding, Some(TextEncoding::Windows1252));
        assert_eq!(
            options.paths,
            vec![PathBuf::from("legacy.p"), PathBuf::from("reference.i")]
        );
        assert!(args(&["--encoding", "guess"]).is_err());
        assert!(args(&["--encoding", "utf8", "--encoding", "cp850"]).is_err());
        assert!(args(&["--unknown"]).is_err());
        assert!(args(&["--help"]).unwrap().is_none());
    }
    #[test]
    fn project_root_is_explicit_and_preserves_native_path() {
        let path = OsString::from("proyecto con espacios");
        let options = parse([OsString::from("--project-root"), path.clone()])
            .unwrap()
            .unwrap();
        assert_eq!(options.project_root, Some(PathBuf::from(path)));
        assert!(args(&["--project-root"]).is_err());
        assert!(args(&["--project-root", "a", "--project-root", "b"]).is_err());
    }
    #[test]
    fn smoke_requires_distinct_absolute_owned_paths() {
        let base = std::env::temp_dir();
        let fixture = base
            .join("jc-gui-parser-fixture.txt")
            .to_string_lossy()
            .into_owned();
        let report = base
            .join("jc-gui-parser-report.txt")
            .to_string_lossy()
            .into_owned();
        let options = args(&["--smoke-fixture", &fixture, "--smoke-report", &report])
            .unwrap()
            .unwrap();
        assert!(options.smoke.is_some());
        let navigation = args(&[
            "--smoke-navigation-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(navigation.smoke.unwrap().navigation_only);
        let line_edit = args(&[
            "--smoke-line-edit-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(line_edit.smoke.unwrap().line_edit_only);
        assert!(args(&["--smoke-line-edit-only"]).is_err());
        assert!(args(&["--smoke-line-edit-only", "--smoke-line-edit-only"]).is_err());
        assert!(args(&["--smoke-navigation-only", "--smoke-line-edit-only"]).is_err());
        assert!(args(&["--smoke-navigation-only"]).is_err());
        assert!(args(&["--smoke-navigation-only", "--smoke-navigation-only"]).is_err());
        assert!(args(&["--smoke-fixture", &fixture]).is_err());
        assert!(args(&["--smoke-report", &report]).is_err());
        assert!(args(&["--smoke-fixture", "relative", "--smoke-report", &report]).is_err());
        assert!(args(&["--smoke-fixture", &fixture, "--smoke-report", &fixture]).is_err());
        assert!(args(&[
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
            "user.p"
        ])
        .is_err());
    }
}

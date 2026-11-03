//! Native frontend CLI; smoke mode operates only on caller-owned fixture paths.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use jc_gui::{GuiOptions, SmokeOptions};
use jc_text::TextEncoding;
use std::ffi::OsString;
use std::path::PathBuf;

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<GuiOptions>, String> {
    let mut options = GuiOptions::default();
    let mut fixture = None;
    let mut report = None;
    let mut bitmap = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
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
    if fixture.is_some() || report.is_some() || bitmap.is_some() {
        if !options.paths.is_empty() {
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
        });
    }
    Ok(Some(options))
}

fn main() {
    match parse(std::env::args_os().skip(1)) {
        Ok(None) => println!("jc-ide-gui [--encoding utf-8|cp1252|latin1|ibm850] [files...]\nWindows native frontend. Smoke: --smoke-fixture ABS --smoke-report ABS [--smoke-bitmap ABS]"),
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

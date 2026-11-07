//! Native frontend CLI; smoke mode operates only on caller-owned fixture paths.
use jc_gui::{GuiOptions, SmokeOptions};
use jc_text::TextEncoding;
use std::ffi::OsString;
use std::path::PathBuf;

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<GuiOptions>, String> {
    let mut options = GuiOptions::default();
    let mut legacy_git_argument = false;
    let mut fixture = None;
    let mut report = None;
    let mut bitmap = None;
    let mut navigation_only = false;
    let mut line_edit_only = false;
    let mut selection_reopen_only = false;
    let mut rectangle_only = false;
    let mut parity_only = false;
    let mut bottom_only = false;
    let mut selector_input_only = false;
    let mut interaction_only = false;
    let mut menu_invoker_only = false;
    let mut syntax_only = false;
    let mut workflow_only = false;
    let mut shell_only = false;
    let mut git_only = false;
    let mut save_only = false;
    let mut performance_only = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(None),
            Some("--smoke-performance-only") => {
                if performance_only {
                    return Err("Opción --smoke-performance-only duplicada".into());
                }
                performance_only = true;
            }
            Some("--smoke-navigation-only") => {
                if navigation_only {
                    return Err("Opción --smoke-navigation-only duplicada".into());
                }
                navigation_only = true;
            }
            Some("--smoke-line-edit-only") => {
                if line_edit_only {
                    return Err("Opción --smoke-line-edit-only duplicada".into());
                }
                line_edit_only = true;
            }
            Some("--git-executable") => {
                if legacy_git_argument {
                    return Err("Opción --git-executable duplicada".into());
                }
                let path = PathBuf::from(args.next().ok_or("Falta la ruta al ejecutable Git")?);
                if !path.is_absolute() || path.as_os_str().len() > 4096 {
                    return Err(
                        "--git-executable requiere una ruta absoluta de hasta4096 unidades nativas"
                            .into(),
                    );
                }
                legacy_git_argument = true; // Accepted only for old launchers; the embedded engine ignores it.
            }
            Some("--smoke-rectangle-only") => {
                if rectangle_only {
                    return Err("Opción --smoke-rectangle-only duplicada".into());
                }
                rectangle_only = true;
            }
            Some("--smoke-parity-only") => {
                if parity_only {
                    return Err("Opción --smoke-parity-only duplicada".into());
                }
                parity_only = true;
            }
            Some("--smoke-bottom-only") => {
                if bottom_only {
                    return Err("Opción --smoke-bottom-only duplicada".into());
                }
                bottom_only = true;
            }
            Some("--smoke-shell-only") => {
                if shell_only {
                    return Err("Opción --smoke-shell-only duplicada".into());
                }
                shell_only = true;
            }
            Some("--smoke-git-only") => {
                if git_only {
                    return Err("Opción --smoke-git-only duplicada".into());
                }
                git_only = true;
            }
            Some("--smoke-save-only") => {
                if save_only {
                    return Err("Opción --smoke-save-only duplicada".into());
                }
                save_only = true;
            }
            Some("--smoke-workflow-only") => {
                if workflow_only {
                    return Err("Opción --smoke-workflow-only duplicada".into());
                }
                workflow_only = true;
            }
            Some("--smoke-syntax-only") => {
                if syntax_only {
                    return Err("Opción --smoke-syntax-only duplicada".into());
                }
                syntax_only = true;
            }
            Some("--smoke-menu-invoker-only") => {
                if menu_invoker_only {
                    return Err("Opción --smoke-menu-invoker-only duplicada".into());
                }
                menu_invoker_only = true;
            }
            Some("--smoke-interaction-only") => {
                if interaction_only {
                    return Err("Opción --smoke-interaction-only duplicada".into());
                }
                interaction_only = true;
            }
            Some("--smoke-selector-input-only") => {
                if selector_input_only {
                    return Err("Opción --smoke-selector-input-only duplicada".into());
                }
                selector_input_only = true;
            }
            Some("--smoke-selection-reopen-only") => {
                if selection_reopen_only {
                    return Err("Opción --smoke-selection-reopen-only duplicada".into());
                }
                selection_reopen_only = true;
            }
            Some("--search-workers") => {
                if options.search_workers.is_some() {
                    return Err("Opción --search-workers duplicada".into());
                }
                let value = args
                    .next()
                    .and_then(|v| v.into_string().ok())
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| (1..=4).contains(n))
                    .ok_or("--search-workers requiere1..4")?;
                options.search_workers = Some(value);
            }
            Some("--project-root") => {
                if options.project_root.is_some() {
                    return Err("Opción --project-root duplicada".into());
                }
                options.project_root = Some(PathBuf::from(
                    args.next().ok_or("Falta la raíz del proyecto")?,
                ));
            }
            Some("--encoding") => {
                if options.encoding.is_some() {
                    return Err("Opción --encoding duplicada".into());
                }
                options.encoding = Some(
                    match args.next().and_then(|s| s.into_string().ok()).as_deref() {
                        Some("utf-8" | "utf8") => TextEncoding::Utf8,
                        Some("cp1252" | "windows-1252") => TextEncoding::Windows1252,
                        Some("latin1" | "iso-8859-1") => TextEncoding::Iso8859_1,
                        Some("ibm850" | "cp850") => TextEncoding::Ibm850,
                        _ => return Err("Usa --encoding utf-8|cp1252|latin1|ibm850".into()),
                    },
                );
            }
            Some("--smoke-fixture" | "--smoke-report" | "--smoke-bitmap") => {
                let path = PathBuf::from(args.next().ok_or("Falta una ruta de prueba")?);
                if !path.is_absolute() {
                    return Err("Las rutas de prueba deben ser absolutas".into());
                }
                let target = match arg.to_str().expect("matched option") {
                    "--smoke-fixture" => &mut fixture,
                    "--smoke-report" => &mut report,
                    _ => &mut bitmap,
                };
                if target.replace(path).is_some() {
                    return Err("Opción de ruta de prueba duplicada".into());
                }
            }
            Some(word) if word.starts_with('-') => {
                return Err(format!("Opción desconocida: {word}"))
            }
            _ => options.paths.push(PathBuf::from(arg)),
        }
    }
    if usize::from(navigation_only)
        + usize::from(line_edit_only)
        + usize::from(selection_reopen_only)
        + usize::from(rectangle_only)
        + usize::from(parity_only)
        + usize::from(bottom_only)
        + usize::from(selector_input_only)
        + usize::from(interaction_only)
        + usize::from(menu_invoker_only)
        + usize::from(syntax_only)
        + usize::from(workflow_only)
        + usize::from(shell_only)
        + usize::from(git_only)
        + usize::from(save_only)
        + usize::from(performance_only)
        > 1
    {
        return Err("Elige una sola sección de prueba".into());
    }
    if fixture.is_some()
        || report.is_some()
        || bitmap.is_some()
        || navigation_only
        || line_edit_only
        || selection_reopen_only
        || rectangle_only
        || parity_only
        || bottom_only
        || selector_input_only
        || interaction_only
        || menu_invoker_only
        || syntax_only
        || workflow_only
        || shell_only
        || git_only
        || save_only
        || performance_only
    {
        if (!performance_only && !options.paths.is_empty()) || options.project_root.is_some() {
            return Err("La prueba usa únicamente su archivo de muestra explícito".into());
        }
        let fixture = fixture.ok_or("--smoke-fixture requires --smoke-report")?;
        let report = report.ok_or("--smoke-report requires --smoke-fixture")?;
        if fixture == report
            || bitmap
                .as_ref()
                .is_some_and(|p| p == &fixture || p == &report)
        {
            return Err("Las rutas de muestra, informe e imagen deben ser distintas".into());
        }
        options.smoke = Some(SmokeOptions {
            fixture,
            report,
            bitmap,
            navigation_only,
            line_edit_only,
            selection_reopen_only,
            rectangle_only,
            parity_only,
            bottom_only,
            selector_input_only,
            interaction_only,
            menu_invoker_only,
            syntax_only,
            workflow_only,
            shell_only,
            git_only,
            save_only,
            performance_only,
        });
    }
    Ok(Some(options))
}

pub(super) fn run() {
    match parse(std::env::args_os().skip(1)) {
        Ok(None) => println!("Interfaz gráfica CIDE\nUso: cide-gui [--project-root DIR] [--search-workers 1..4] [--encoding utf-8|cp1252|latin1|ibm850] [ARCHIVOS...]\nAlias anterior: jc-ide-gui. Interfaz nativa de Windows. Prueba: --smoke-fixture ABS --smoke-report ABS [--smoke-bitmap ABS]"),
        Ok(Some(options)) => {
            if let Err(error) = jc_gui::run(options) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}; usa --help");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_git_argument_is_consumed_without_configuring_an_external_engine() {
        let path = std::env::current_dir()
            .unwrap()
            .join("trusted Git that need not exist.exe");
        let options = super::parse([
            std::ffi::OsString::from("--git-executable"),
            path.as_os_str().into(),
        ])
        .unwrap()
        .unwrap();
        assert!(options.project_root.is_none());
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
        let selection_reopen = args(&[
            "--smoke-selection-reopen-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(selection_reopen.smoke.unwrap().selection_reopen_only);
        let rectangle = args(&[
            "--smoke-rectangle-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(rectangle.smoke.unwrap().rectangle_only);
        let parity = args(&[
            "--smoke-parity-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(parity.smoke.unwrap().parity_only);
        let bottom = args(&[
            "--smoke-bottom-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(bottom.smoke.unwrap().bottom_only);
        let selector_input = args(&[
            "--smoke-selector-input-only",
            "--smoke-fixture",
            &fixture,
            "--smoke-report",
            &report,
        ])
        .unwrap()
        .unwrap();
        assert!(selector_input.smoke.unwrap().selector_input_only);
        assert!(args(&["--smoke-selector-input-only"]).is_err());
        assert!(args(&["--smoke-selector-input-only", "--smoke-selector-input-only"]).is_err());
        assert!(args(&["--smoke-selector-input-only", "--smoke-bottom-only"]).is_err());
        assert!(args(&["--smoke-bottom-only"]).is_err());
        assert!(args(&["--smoke-bottom-only", "--smoke-bottom-only"]).is_err());
        assert!(args(&["--smoke-bottom-only", "--smoke-parity-only"]).is_err());
        assert!(args(&["--smoke-parity-only"]).is_err());
        assert!(args(&["--smoke-parity-only", "--smoke-parity-only"]).is_err());
        assert!(args(&["--smoke-parity-only", "--smoke-rectangle-only"]).is_err());
        assert!(args(&["--smoke-rectangle-only"]).is_err());
        assert!(args(&["--smoke-rectangle-only", "--smoke-rectangle-only"]).is_err());
        assert!(args(&["--smoke-line-edit-only", "--smoke-rectangle-only"]).is_err());
        assert!(args(&["--smoke-selection-reopen-only"]).is_err());
        assert!(args(&[
            "--smoke-selection-reopen-only",
            "--smoke-selection-reopen-only"
        ])
        .is_err());
        assert!(args(&["--smoke-line-edit-only", "--smoke-selection-reopen-only"]).is_err());
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

#[cfg(test)]
mod interaction_options_tests {
    use super::parse;
    use std::ffi::OsString;
    #[test]
    fn save_section_requires_explicit_paths_and_exclusive_selection() {
        let parse_args = |words: &[&str]| parse(words.iter().map(OsString::from));
        assert!(parse_args(&["--smoke-save-only"]).is_err());
        assert!(parse_args(&["--smoke-save-only", "--smoke-git-only"]).is_err());
        assert!(parse_args(&["--smoke-save-only", "--smoke-save-only"]).is_err());
        let root = std::env::current_dir().unwrap();
        let fixture = root.join("fixture.txt").into_os_string();
        let report = root.join("report.json").into_os_string();
        assert!(parse([
            OsString::from("--smoke-save-only"),
            OsString::from("--smoke-fixture"),
            fixture.clone(),
        ])
        .is_err());
        assert!(parse([
            OsString::from("--smoke-save-only"),
            OsString::from("--smoke-report"),
            report.clone(),
        ])
        .is_err());
        let options = parse([
            OsString::from("--smoke-save-only"),
            OsString::from("--smoke-fixture"),
            fixture,
            OsString::from("--smoke-report"),
            report,
        ])
        .unwrap()
        .unwrap();
        assert!(options.smoke.unwrap().save_only);
    }
    #[test]
    fn git_section_requires_fixture_report_and_exclusive_selection() {
        let parse_args = |words: &[&str]| parse(words.iter().map(OsString::from));
        assert!(parse_args(&["--smoke-git-only"]).is_err());
        assert!(parse_args(&["--smoke-git-only", "--smoke-shell-only"]).is_err());
        assert!(parse_args(&["--smoke-git-only", "--smoke-git-only"]).is_err());
        let root = std::env::current_dir().unwrap();
        let fixture = root.join("fixture.txt").into_os_string();
        let report = root.join("report.json").into_os_string();
        assert!(parse([
            OsString::from("--smoke-git-only"),
            OsString::from("--smoke-fixture"),
            fixture.clone(),
        ])
        .is_err());
        assert!(parse([
            OsString::from("--smoke-git-only"),
            OsString::from("--smoke-report"),
            report.clone(),
        ])
        .is_err());
        let options = parse([
            OsString::from("--smoke-git-only"),
            OsString::from("--smoke-fixture"),
            fixture,
            OsString::from("--smoke-report"),
            report,
        ])
        .unwrap()
        .unwrap();
        assert!(options.smoke.unwrap().git_only);
    }
    #[test]
    fn interaction_section_requires_owned_paths_and_is_exclusive() {
        let parse_args = |words: &[&str]| parse(words.iter().map(OsString::from));
        assert!(parse_args(&["--smoke-interaction-only"]).is_err());
        assert!(parse_args(&["--smoke-interaction-only", "--smoke-bottom-only"]).is_err());
        assert!(parse_args(&["--smoke-interaction-only", "--smoke-interaction-only"]).is_err());
        let root = std::env::current_dir().unwrap();
        let fixture = root.join("fixture.txt");
        let report = root.join("report.json");
        let args = vec![
            OsString::from("--smoke-interaction-only"),
            OsString::from("--smoke-fixture"),
            fixture.into_os_string(),
            OsString::from("--smoke-report"),
            report.into_os_string(),
        ];
        assert!(
            parse(args)
                .unwrap()
                .unwrap()
                .smoke
                .unwrap()
                .interaction_only
        );
    }
}

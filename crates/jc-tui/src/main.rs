use jc_text::TextEncoding;
use jc_tui::LaunchOptions;
use std::env;
use std::path::PathBuf;

fn main() {
    match parse_args(env::args().skip(1)) {
        Ok(Some(options)) => {
            if let Err(error) =
                jc_tui::run_with_search_workers(options.launch, options.search_workers)
            {
                eprintln!("jc-ide-tui: {error}");
                std::process::exit(1);
            }
        }
        Ok(None) => print_help(),
        Err(error) => {
            eprintln!("jc-ide-tui: {error}\n");
            print_help();
            std::process::exit(2);
        }
    }
}

struct Parsed {
    launch: LaunchOptions,
    search_workers: usize,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Parsed>, String> {
    let mut options = LaunchOptions::default();
    let mut search_workers = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--encoding" => {
                let value = args.next().ok_or("--encoding requires a value")?;
                options.encoding = Some(parse_encoding(&value)?);
            }
            "--search-workers" => {
                if search_workers.is_some() {
                    return Err("--search-workers may be specified only once".into());
                }
                let value = args
                    .next()
                    .ok_or("--search-workers requires a value in 1..4")?;
                search_workers = Some(match value.as_str() {
                    "1" => 1,
                    "2" => 2,
                    "3" => 3,
                    "4" => 4,
                    _ => return Err("--search-workers requires an integer in 1..4".into()),
                });
            }
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            path => options.files.push(PathBuf::from(path)),
        }
    }
    Ok(Some(Parsed {
        launch: options,
        search_workers: search_workers.unwrap_or(2),
    }))
}

fn parse_encoding(value: &str) -> Result<TextEncoding, String> {
    match value.to_ascii_lowercase().as_str() {
        "utf8" | "utf-8" => Ok(TextEncoding::Utf8),
        "windows-1252" | "cp1252" | "1252" => Ok(TextEncoding::Windows1252),
        "iso-8859-1" | "iso8859-1" | "latin1" => Ok(TextEncoding::Iso8859_1),
        "ibm850" | "cp850" | "850" => Ok(TextEncoding::Ibm850),
        _ => Err(format!("unsupported encoding: {value}")),
    }
}

fn print_help() {
    println!(
        "JC-IDE TUI\n\
Usage: jc-ide-tui [--encoding NAME] [--search-workers 1..4] [FILE ...]\n\
Encodings: utf-8, windows-1252, iso-8859-1, ibm850\n\
Search workers: default 2; total includes the coordinator.\n\
Effective count is capped to available logical CPUs minus 4, clamped to 1..4.\n\
More workers may increase total CPU; other running workloads are not inspected.\n\
Inside the TUI use Alt+F for File, Ctrl+Shift+P for the command palette, F6 to cycle focus."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_aliases_are_explicit() {
        assert_eq!(parse_encoding("cp850").unwrap(), TextEncoding::Ibm850);
        assert_eq!(parse_encoding("1252").unwrap(), TextEncoding::Windows1252);
        assert!(parse_encoding("guess").is_err());
    }

    #[test]
    fn parser_keeps_multiple_files() {
        let options = parse_args(["--encoding", "utf-8", "a.p", "b.cs"].map(str::to_owned))
            .unwrap()
            .unwrap();
        assert_eq!(options.launch.encoding, Some(TextEncoding::Utf8));
        assert_eq!(options.search_workers, 2);
        assert_eq!(
            options.launch.files,
            vec![PathBuf::from("a.p"), PathBuf::from("b.cs")]
        );
    }

    #[test]
    fn search_workers_default_and_explicit_counts_preserve_launch_arguments() {
        let default = parse_args(std::iter::empty::<String>()).unwrap().unwrap();
        assert_eq!(default.search_workers, 2);
        assert!(default.launch.files.is_empty());
        for workers in 1..=4 {
            let options = parse_args([
                "first file.p".to_owned(),
                "--search-workers".into(),
                workers.to_string(),
                "--encoding".into(),
                "cp1252".into(),
                "last.cs".into(),
            ])
            .unwrap()
            .unwrap();
            assert_eq!(options.search_workers, workers);
            assert_eq!(options.launch.encoding, Some(TextEncoding::Windows1252));
            assert_eq!(
                options.launch.files,
                vec![PathBuf::from("first file.p"), PathBuf::from("last.cs")]
            );
        }
    }

    #[test]
    fn search_workers_reject_invalid_missing_and_duplicate_values() {
        for value in ["0", "5", "-1", "two", "1.5", ""] {
            assert!(parse_args(["--search-workers", value].map(str::to_owned)).is_err());
        }
        assert!(parse_args(["--search-workers"].map(str::to_owned)).is_err());
        assert!(parse_args(["--search-workers", "--encoding", "utf8"].map(str::to_owned)).is_err());
        assert!(parse_args(
            ["--search-workers", "1", "a.p", "--search-workers", "2",].map(str::to_owned)
        )
        .is_err());
    }

    #[test]
    fn help_still_short_circuits_launch_and_unknown_flags_are_rejected() {
        for help in ["-h", "--help"] {
            assert!(
                parse_args(["--search-workers", "1", help].map(str::to_owned))
                    .unwrap()
                    .is_none()
            );
        }
        assert!(parse_args(["--search-worker", "2"].map(str::to_owned)).is_err());
    }
}

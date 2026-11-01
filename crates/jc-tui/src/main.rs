use jc_text::TextEncoding;
use jc_tui::LaunchOptions;
use std::env;
use std::path::PathBuf;

fn main() {
    match parse_args(env::args().skip(1)) {
        Ok(Some(options)) => {
            if let Err(error) = jc_tui::run(options) {
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

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<LaunchOptions>, String> {
    let mut options = LaunchOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--encoding" => {
                let value = args.next().ok_or("--encoding requires a value")?;
                options.encoding = Some(parse_encoding(&value)?);
            }
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            path => options.files.push(PathBuf::from(path)),
        }
    }
    Ok(Some(options))
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
Usage: jc-ide-tui [--encoding NAME] [FILE ...]\n\
Encodings: utf-8, windows-1252, iso-8859-1, ibm850\n\
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
        assert_eq!(options.encoding, Some(TextEncoding::Utf8));
        assert_eq!(options.files, vec![PathBuf::from("a.p"), PathBuf::from("b.cs")]);
    }
}

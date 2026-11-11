use jc_core::Document;
use jc_progress::{
    build_command, parse_response, Action, BridgeRequest, CompletionStatus, ProgressProfile,
};
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub(super) fn entrypoint() {
    if let Err(error) = run(env::args_os().skip(1).collect()) {
        eprintln!("cide: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<OsString>) -> Result<(), Box<dyn Error>> {
    let command = args
        .first()
        .and_then(|value| value.to_str())
        .unwrap_or("help");
    match command {
        "help" | "--help" | "-h" => help(),
        "languages" if args.len() == 1 => {
            for profile in jc_languages::PROFILES {
                println!(
                    "{:<12} {:<24} {}",
                    profile.id,
                    profile.name,
                    profile.extensions.join(", ")
                );
            }
            println!("Profiles identify files; syntax engines and LSP are not yet implemented.");
        }
        "inspect" if args.len() == 2 => {
            let path = Path::new(&args[1]);
            let original = fs::read(path)?;
            let utf8_valid = std::str::from_utf8(&original).is_ok();
            let nul_count = original.iter().filter(|byte| **byte == 0).count();
            let doc = Document::from_bytes(original);
            println!("File: {}", path.display());
            println!(
                "Language hint: {}",
                jc_languages::detect(&path.to_string_lossy()).id
            );
            println!("Bytes: {}", doc.len());
            println!("NUL bytes: {nul_count}");
            println!("Valid UTF-8 byte sequence: {utf8_valid} (not an encoding selection)");
            println!("Modified: {}", doc.is_modified());
            println!("Source bytes have not been decoded or rewritten.");
        }
        "roundtrip" if args.len() == 3 => {
            let doc = Document::from_bytes(fs::read(&args[1])?);
            save_new(&doc, Path::new(&args[2]))?;
            println!(
                "Wrote {} unchanged bytes to {}",
                doc.len(),
                Path::new(&args[2]).display()
            );
        }
        "edit" if args.len() == 6 => {
            let start = text_arg(&args[2])?.parse::<usize>()?;
            let end = text_arg(&args[3])?.parse::<usize>()?;
            let inserted = parse_hex(text_arg(&args[4])?)?;
            let mut doc = Document::from_bytes(fs::read(&args[1])?);
            doc.replace(start..end, &inserted)?;
            save_new(&doc, Path::new(&args[5]))?;
            println!(
                "Wrote edited copy ({} bytes); the original was not overwritten.",
                doc.len()
            );
        }
        "demo" if args.len() == 1 => {
            let original = b"DISPLAY \"ni\xf1o\".\r\n".to_vec();
            let mut doc = Document::from_bytes(original.clone());
            doc.insert(0, b"/* CIDE */\r\n")?;
            let edited = doc.to_bytes();
            assert!(doc.undo());
            assert_eq!(doc.to_bytes(), original);
            assert!(doc.redo());
            assert_eq!(doc.to_bytes(), edited);
            println!("Byte editing, undo and redo preserve the non-UTF-8 byte F1 and CRLF.");
            println!("This is a headless core demonstration, not a graphical editor.");
        }
        "progress-plan" => progress_plan(&args[1..])?,
        "progress-result" if args.len() == 2 => {
            let mut bytes = Vec::new();
            fs::File::open(&args[1])?
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            let response = parse_response(&bytes)?;
            println!("Status: {:?}", response.status);
            println!("CPINTERNAL: {}", response.cpinternal);
            println!("CPSTREAM: {}", response.cpstream);
            println!("Diagnostics: {}", response.diagnostics.len());
            println!("Schema records: {}", response.schema.len());
            if response.status != CompletionStatus::Ok {
                return Err(invalid(
                    "Progress reported a failed operation; inspect the retained response",
                )
                .into());
            }
        }
        _ => return Err(invalid("unknown command or wrong argument count; use --help").into()),
    }
    Ok(())
}

fn save_new(doc: &Document, path: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    doc.write_to(&mut file)?;
    file.sync_all()
}

fn parse_hex(value: &str) -> Result<Vec<u8>, io::Error> {
    if value == "-" {
        return Ok(Vec::new());
    }
    if !value.is_ascii() || value.len() % 2 != 0 {
        return Err(invalid(
            "replacement must be an even-length hexadecimal string, or - to delete",
        ));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("ASCII checked above");
            u8::from_str_radix(text, 16)
                .map_err(|_| invalid("replacement contains a non-hexadecimal digit"))
        })
        .collect()
}

fn text_arg(value: &OsString) -> Result<&str, io::Error> {
    value
        .to_str()
        .ok_or_else(|| invalid("this option requires a Unicode argument"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn progress_plan(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    if args.len() % 2 != 0 {
        return Err(invalid("progress-plan requires --name value pairs").into());
    }
    let allowed = [
        "--action",
        "--source",
        "--output-dir",
        "--exe",
        "--pf",
        "--ini",
        "--cpstream",
        "--cpinternal",
        "--propath",
        "--helper",
    ];
    let mut options = BTreeMap::new();
    for pair in args.chunks_exact(2) {
        let key = text_arg(&pair[0])?;
        if !allowed.contains(&key) || options.insert(key, text_arg(&pair[1])?).is_some() {
            return Err(invalid("unknown or duplicate progress-plan option").into());
        }
    }
    let required = |key: &str| -> Result<&str, io::Error> {
        options
            .get(key)
            .copied()
            .ok_or_else(|| invalid(&format!("missing {key}")))
    };
    let action = match required("--action")? {
        "check" => Action::Check,
        "compile" => Action::Compile,
        "run" => Action::Run,
        "schema" => Action::Schema,
        _ => return Err(invalid("action must be check, compile, run or schema").into()),
    };
    let profile = ProgressProfile {
        executable: PathBuf::from(required("--exe")?),
        parameter_file: options.get("--pf").map(PathBuf::from),
        ini_file: options.get("--ini").map(PathBuf::from),
        cpstream: required("--cpstream")?.to_owned(),
        cpinternal: required("--cpinternal")?.to_owned(),
    };
    let request = BridgeRequest {
        action,
        source: options.get("--source").map(PathBuf::from),
        output_dir: PathBuf::from(required("--output-dir")?),
        propath: options.get("--propath").map(|value| value.to_string()),
    };
    if !request.output_dir.is_absolute() {
        return Err(
            invalid("--output-dir must be absolute on the current operating system").into(),
        );
    }
    let request_bytes = request.encode()?;
    let request_file = request.output_dir.join("request.txt");
    let spec = build_command(&profile, Path::new(required("--helper")?), &request_file)?;
    fs::create_dir(&request.output_dir)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request_file)?;
    file.write_all(&request_bytes)?;
    file.sync_all()?;
    println!("Prepared request: {}", request_file.display());
    println!("Expected response: {}", request.response_path().display());
    println!("Executable: {:?}", spec.executable);
    for (index, argument) in spec.args.iter().enumerate() {
        println!("Argument {index}: {argument:?}");
    }
    println!("Plan only: Progress was not started. ABL helpers require validation on the target installation.");
    Ok(())
}

fn help() {
    println!(
        "CIDE 0.1.0 — headless CLI; native GUI available separately as cide-gui\n\
Canonical command: cide (legacy alias: jc-ide).\n\
Commands:\n\
  cide languages\n\
  cide inspect FILE\n\
  cide roundtrip INPUT NEW_OUTPUT\n\
  cide edit INPUT START_BYTE END_BYTE HEX_OR_DASH NEW_OUTPUT\n\
  cide demo\n\
  cide progress-result RESPONSE_FILE\n\
  cide progress-plan --action check|compile|run|schema --exe PATH --helper PATH\n\
      --output-dir NEW_DIR --cpstream PAGE --cpinternal PAGE\n\
      [--source FILE] [--pf FILE] [--ini FILE] [--propath PATHS]\n\
All edit offsets are bytes. Existing output files are never overwritten.\n\
Progress commands prepare requests only; no database or program is accessed."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexadecimal_input_never_performs_lossy_text_conversion() {
        assert_eq!(parse_hex("00ffF180").unwrap(), vec![0, 255, 241, 128]);
        assert_eq!(parse_hex("-").unwrap(), Vec::<u8>::new());
        for invalid in ["f", "ñ", "gg", "00 11"] {
            assert!(parse_hex(invalid).is_err());
        }
    }
}

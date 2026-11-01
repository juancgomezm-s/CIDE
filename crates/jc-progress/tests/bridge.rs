use jc_progress::{
    build_command, parse_response, Action, BridgeRequest, CompletionStatus, ProgressProfile,
    SchemaRecord, Severity,
};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const HEADER: &str = "JCIDE|1|49534F383835392D31|49424D383530\n";

#[test]
fn paths_with_spaces_remain_single_arguments_and_no_shell_is_used() {
    let profile = ProgressProfile {
        executable: PathBuf::from(r"C:\Program Files\Progress\bin\prowin32.exe"),
        parameter_file: Some(PathBuf::from(r"C:\ERP Test\startup.pf")),
        ini_file: Some(PathBuf::from(r"C:\ERP Test\progress.ini")),
        cpstream: "IBM850".into(),
        cpinternal: "ISO8859-1".into(),
    };
    let spec = build_command(
        &profile,
        Path::new(r"C:\JC IDE\jc-bridge.p"),
        Path::new(r"C:\JC IDE\job 1,dev\request.txt"),
    ).unwrap();
    assert_eq!(spec.executable, profile.executable);
    assert_eq!(spec.args[1], OsString::from(r"C:\ERP Test\progress.ini"));
    assert_eq!(spec.args[3], OsString::from(r"C:\ERP Test\startup.pf"));
    assert_eq!(spec.args.last().unwrap(), &OsString::from(r"C:\JC IDE\job 1,dev\request.txt"));
    let command = spec.to_command();
    assert_eq!(command.get_program(), profile.executable);
    assert_eq!(command.get_args().collect::<Vec<_>>(), spec.args.iter().map(|s| s.as_os_str()).collect::<Vec<_>>());
}

#[test]
fn request_does_not_mix_path_commas_with_parameter_fields() {
    let request = BridgeRequest {
        action: Action::Check,
        source: Some(PathBuf::from(r"C:\source, old\module.p")),
        output_dir: PathBuf::from(r"C:\JC IDE\jobs\one"),
        propath: Some(r"C:\ERP,C:\ERP\includes".into()),
    };
    assert_eq!(request.encode().unwrap(), b"JCIDE/1\ncheck\nC:\\source, old\\module.p\nC:\\JC IDE\\jobs\\one\nC:\\ERP,C:\\ERP\\includes\n");
}

#[test]
fn rejects_unknown_or_ambiguous_request_transport_inputs() {
    let mut request = BridgeRequest {
        action: Action::Check,
        source: None,
        output_dir: PathBuf::from("/tmp/job"),
        propath: None,
    };
    assert!(request.encode().is_err());
    request.source = Some(PathBuf::from("relative.p"));
    assert!(request.encode().is_err());
    request.source = Some(PathBuf::from("/src/año.p"));
    assert!(request.encode().is_err());
    request.source = Some(PathBuf::from("/src/a.p"));
    request.propath = Some("/src\ncompile".into());
    assert!(request.encode().is_err());
    request.propath = None;
    request.action = Action::Schema;
    assert!(request.encode().is_err());
    request.source = None;
    assert!(request.encode().is_ok());
}

#[test]
fn code_pages_are_mandatory_and_not_silently_defaulted() {
    let mut profile = ProgressProfile {
        executable: PathBuf::from("/opt/progress/bin/_progres"),
        parameter_file: None,
        ini_file: None,
        cpstream: "".into(),
        cpinternal: "ISO8859-1".into(),
    };
    assert!(build_command(&profile, Path::new("/jc/bridge.p"), Path::new("/tmp/request")).is_err());
    profile.cpstream = "IBM850 -p other.p".into();
    assert!(build_command(&profile, Path::new("/jc/bridge.p"), Path::new("/tmp/request")).is_err());
    profile.cpstream = "IBM850".into();
    assert!(build_command(&profile, Path::new("/jc/bridge.p"), Path::new("/tmp/request")).is_ok());
}

#[test]
fn legacy_bytes_and_record_delimiters_survive_response_parsing() {
    let source = format!("{HEADER}DIAG|error|7|3|433A5C612E70|E1FF007C0A\nEND|ERROR\n");
    let result = parse_response(source.as_bytes()).unwrap();
    assert_eq!(result.cpinternal, "ISO8859-1");
    assert_eq!(result.cpstream, "IBM850");
    assert_eq!(result.status, CompletionStatus::Error);
    assert_eq!(result.diagnostics[0].severity, Severity::Error);
    assert_eq!(result.diagnostics[0].message.as_bytes(), &[0xe1, 0xff, 0, b'|', b'\n']);
    assert_eq!(result.diagnostics[0].row, Some(7));
}

#[test]
fn schema_carries_fields_indexes_and_distinguishes_unknown_from_empty() {
    let source = format!("{HEADER}TABLE|6462|54\nFIELD|6462|54|46|434841524143544552|2||-\nINDEX|6462|54|49|1|1\nINDEX_FIELD|6462|54|49|1|46|0\nEND|OK\n");
    let result = parse_response(source.as_bytes()).unwrap();
    assert_eq!(result.schema.len(), 4);
    match &result.schema[1] {
        SchemaRecord::Field { extent, format, label, .. } => {
            assert_eq!(*extent, 2);
            assert_eq!(format.as_ref().unwrap().as_bytes(), b"");
            assert_eq!(*label, None);
        }
        _ => panic!("expected field"),
    }
    assert!(matches!(&result.schema[3], SchemaRecord::IndexField { ascending: false, sequence: 1, .. }));
}

#[test]
fn incomplete_malformed_or_contradictory_results_are_never_successful() {
    for body in [
        "",
        "TABLE|61|62\n",
        "END|OK\nTABLE|61|62\n",
        "END|OK\nEND|OK\n",
        "DIAG|error|-|-|-|61\nEND|OK\n",
        "DIAG|error|0|1|-|61\nEND|ERROR\n",
        "DIAG|error|-|-|-|GG\nEND|ERROR\n",
        "FIELD|61|62|63|64|x||\nEND|OK\n",
        "TABLE|6|62\nEND|OK\n",
        "INDEX|61|62|63|2|0\nEND|OK\n",
        "TABLE|61|62|extra\nEND|OK\n",
        "UNKNOWN|a\nEND|OK\n",
        "DIAG|error|999999999999999|1|-|61\nEND|ERROR\n",
    ] {
        assert!(parse_response(format!("{HEADER}{body}").as_bytes()).is_err(), "accepted {body:?}");
    }
    assert!(parse_response(b"JCIDE|2|61|62\nEND|OK\n").is_err());
    assert!(parse_response(b"JCIDE|1|61|62\nDIAG|error|-|-|-|\xff\nEND|ERROR\n").is_err());
}

#[test]
fn windows_line_endings_and_missing_positions_are_supported() {
    let source = format!("{HEADER}DIAG|warning|-|-|-|616263\nEND|OK\n").replace('\n', "\r\n");
    let result = parse_response(source.as_bytes()).unwrap();
    assert_eq!(result.diagnostics[0].row, None);
    assert_eq!(result.diagnostics[0].file, None);
    assert_eq!(result.status, CompletionStatus::Ok);
}

#[test]
fn adversarial_headers_are_bounded_before_field_allocation() {
    let oversized = format!("{}\nEND|OK\n", "|".repeat(128 * 1024 + 1));
    let error = parse_response(oversized.as_bytes()).unwrap_err();
    assert_eq!(error.line, Some(1));
    assert_eq!(error.message, "record exceeds 128 KiB");
    let delimiters = format!("JCIDE|1|{}\nEND|OK\n", "|".repeat(64 * 1024));
    assert!(parse_response(delimiters.as_bytes()).is_err());
    let record = format!("{HEADER}FIELD|{}\nEND|OK\n", "|".repeat(64 * 1024));
    assert!(parse_response(record.as_bytes()).is_err());
}

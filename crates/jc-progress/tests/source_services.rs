#![cfg(feature = "source-services")]
use jc_app::{
    source_services::{
        reference_candidates, SourceAction, SourceController, SourceMode, SourceRegistry,
        SourceState,
    },
    JobSystem, Wakeup, Workbench,
};
use jc_text::{encode, TextEncoding};
use std::time::{Duration, Instant};
fn run(text: &str, encoding: TextEncoding, mode: SourceMode) -> (Workbench, SourceController) {
    let mut a = Workbench::default();
    a.new_file("test.p", encoding);
    a.insert_text(text).unwrap();
    let mut r = SourceRegistry::default();
    r.register(jc_progress::source_services::descriptor())
        .unwrap();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(mode, &r, &a, None, &mut jobs).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while jobs.outstanding() > 0 || matches!(c.state(), SourceState::Queued | SourceState::Running)
    {
        for mut task in jobs.poll(&a, 4) {
            c.accept(&a, None, &mut task);
        }
        c.tick(&a, None, &mut jobs);
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    (a, c)
}
#[test]
fn abl_symbols_ignore_comments_strings_preprocessor_and_end_statements_in_all_encodings() {
    let text="/* PROCEDURE fake: /* FUNCTION nope: */ */\r\nDISPLAY \"PROCEDURE hidden: RUN hidden.p.\".\r\n&SCOPED-DEFINE x PROCEDURE skipped:\r\nPROCEDURE niño:\r\nEND PROCEDURE.\r\nFUNCTION compute RETURNS INTEGER():\r\nEND FUNCTION.\r\nCLASS Example:\r\nMETHOD PUBLIC VOID work():\r\nEND METHOD.\r\nEND CLASS.";
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let (mut a, mut c) = run(text, encoding, SourceMode::Symbols);
        assert_eq!(c.state(), SourceState::Ready);
        let labels: Vec<_> = c.items().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "procedure niño (lexical)",
                "function compute (lexical)",
                "class Example (lexical)",
                "method work (lexical)"
            ]
        );
        assert_eq!(
            &a.active_bytes().unwrap()[c.items()[0].range.clone()],
            encode("niño", encoding).unwrap()
        );
        let before = a.active_bytes().unwrap();
        c.activate(&mut a, None, false).unwrap();
        assert_eq!(a.active_bytes().unwrap(), before);
    }
}
#[test]
fn abl_references_distinguish_static_local_and_dynamic_remote_context_without_executing() {
    let text="PROCEDURE work:\nEND PROCEDURE.\n{common.i &x=1}\nRUN target.p.\nRUN \"quoted.p\".\nRUN work.\nRUN work IN otherHandle.\nRUN remote.p ON SERVER remoteHandle.\nRUN VALUE(which).\n/* RUN excluded.p. */\nDISPLAY \"RUN hidden.p.\".";
    let (a, c) = run(text, TextEncoding::Utf8, SourceMode::References);
    assert_eq!(c.state(), SourceState::Ready);
    assert_eq!(c.items().len(), 7);
    assert_eq!(
        c.items()[0].action,
        SourceAction::RelativeFile("common.i".into())
    );
    assert_eq!(
        c.items()[1].action,
        SourceAction::RelativeFile("target.p".into())
    );
    assert_eq!(
        c.items()[2].action,
        SourceAction::RelativeFile("quoted.p".into())
    );
    assert_eq!(c.items()[3].action, SourceAction::Navigate);
    assert!(c.items()[4..]
        .iter()
        .all(|i| matches!(i.action, SourceAction::Unresolved(_))));
    assert_eq!(a.active_bytes().unwrap(), text.as_bytes());
}
#[test]
fn abl_source_chunk_boundaries_nested_comments_and_result_limits_are_explicit() {
    let text = format!(
        "/*{}*/\nPROCEDURE visible:\nEND PROCEDURE.",
        "x".repeat(4094)
    );
    let (_, c) = run(&text, TextEncoding::Utf8, SourceMode::Symbols);
    assert_eq!(c.state(), SourceState::Ready);
    assert_eq!(c.items().len(), 1);
    use std::fmt::Write;
    let mut many = String::new();
    for i in 0..65 {
        write!(&mut many, "PROCEDURE p{i}:\nEND PROCEDURE.\n").unwrap();
    }
    let (_, c) = run(&many, TextEncoding::Utf8, SourceMode::Symbols);
    assert_eq!(c.state(), SourceState::Limited);
    assert!(c.items().is_empty());
}
#[test]
fn static_reference_candidates_are_confined_ambiguous_and_not_a_propath_emulator() {
    let base = std::env::temp_dir().join(format!("cide-source-candidates-{}", std::process::id()));
    std::fs::create_dir(&base).unwrap();
    let root = base.join("project");
    std::fs::create_dir(&root).unwrap();
    let sub = root.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let source = sub.join("source.p");
    let local = sub.join("target.p");
    let shared = root.join("target.p");
    let outside = base.join("outside.p");
    for p in [&source, &local, &shared, &outside] {
        std::fs::write(p, b"/* owned source */\r\n").unwrap();
    }
    let paths = reference_candidates(&root, &source, "target.p").unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], local.canonicalize().unwrap());
    assert_eq!(paths[1], shared.canonicalize().unwrap());
    assert!(reference_candidates(&root, &source, "../../outside.p")
        .unwrap()
        .is_empty());
    assert!(reference_candidates(&root, &source, &outside.display().to_string()).is_err());
    assert!(reference_candidates(&root, &source, "missing.p")
        .unwrap()
        .is_empty());
    for p in [&source, &local, &shared, &outside] {
        std::fs::remove_file(p).unwrap();
    }
    std::fs::remove_dir(sub).unwrap();
    std::fs::remove_dir(root).unwrap();
    std::fs::remove_dir(base).unwrap();
}

#[test]
fn qualified_and_fragmented_names_preserve_full_ranges_instead_of_return_types_or_suffix_bytes() {
    let (_, controller) = run(
        "CLASS Example.Tools.Worker:\nEND CLASS.",
        TextEncoding::Utf8,
        SourceMode::Symbols,
    );
    assert_eq!(
        controller.items()[0].label,
        "class Example.Tools.Worker (lexical)"
    );
    let text=format!("FUNCTION {} RETURNS CustomType():\nEND FUNCTION.\nMETHOD PUBLIC SomeType {}():\nEND METHOD.","a".repeat(100),"b".repeat(100));
    let (_, controller) = run(&text, TextEncoding::Utf8, SourceMode::Symbols);
    assert_eq!(controller.state(), SourceState::Ready);
    assert_eq!(controller.items().len(), 2);
    assert_eq!(
        controller.items()[0].label,
        format!("function {} (lexical)", "a".repeat(100))
    );
    assert_eq!(
        controller.items()[1].label,
        format!("method {} (lexical)", "b".repeat(100))
    );
    assert_eq!(
        &text.as_bytes()[controller.items()[0].range.clone()],
        b"a".repeat(100)
    );
    assert_eq!(
        &text.as_bytes()[controller.items()[1].range.clone()],
        b"b".repeat(100)
    );
}

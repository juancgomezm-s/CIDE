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
    let c = scan(&a, None, mode);
    (a, c)
}
fn scan(a: &Workbench, root: Option<&std::path::Path>, mode: SourceMode) -> SourceController {
    let mut r = SourceRegistry::default();
    r.register(jc_progress::source_services::descriptor())
        .unwrap();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(mode, &r, a, root, &mut jobs).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while jobs.outstanding() > 0 || matches!(c.state(), SourceState::Queued | SourceState::Running)
    {
        for mut task in jobs.poll(a, 4) {
            c.accept(a, root, &mut task);
        }
        c.tick(a, root, &mut jobs);
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    c
}

#[test]
fn abl_routine_relationships_group_local_callers_and_follow_the_active_procedure() {
    let source = "PROCEDURE principal:\n  RUN Saludo.\n  RUN saludo.\n  DISPLAY cantidad().\nEND PROCEDURE.\nPROCEDURE Saludo:\nEND PROCEDURE.\nFUNCTION cantidad RETURNS INTEGER():\n  RETURN 2.\nEND FUNCTION.\nPROCEDURE otro:\n  RUN saludo.\nEND PROCEDURE.";
    let mut app = Workbench::default();
    app.new_file("relaciones.p", TextEncoding::Utf8);
    app.insert_text(source).unwrap();
    let at = source.find("principal").unwrap();
    app.set_linear_selection_bytes(at, at).unwrap();
    let outgoing = scan(&app, None, SourceMode::Calls);
    let names: Vec<_> = outgoing
        .items()
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(names, ["Saludo", "cantidad"], "{}", outgoing.message());
    let at = source.find("PROCEDURE Saludo").unwrap();
    app.set_linear_selection_bytes(at, at).unwrap();
    let incoming = scan(&app, None, SourceMode::Incoming);
    let names: Vec<_> = incoming
        .items()
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(names, ["principal", "otro"], "{}", incoming.message());
}

#[test]
fn abl_outgoing_tree_pins_three_external_files_and_rejects_changed_destinations() {
    use std::fs;
    let root = std::env::temp_dir().join(format!(
        "cide-tree-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for (name, code) in [
        ("a.p", "RUN b.p."),
        ("b.p", "RUN c.p."),
        ("c.p", "RUN d.p."),
        ("d.p", "RUN e.p."),
        ("e.p", "RETURN."),
    ] {
        fs::write(root.join(name), code).unwrap();
    }
    let long_source = format!("/*{}*/\nRUN e.p.", "x".repeat(3 * 1024 * 1024));
    fs::write(root.join("d.p"), &long_source).unwrap();
    let mut app = Workbench::default();
    app.open_file(root.join("a.p"), Some(TextEncoding::Utf8))
        .unwrap();
    let c = scan(&app, Some(&root), SourceMode::Calls);
    assert_eq!(c.state(), SourceState::Ready, "{}", c.message());
    assert_eq!(c.items().len(), 3);
    assert_eq!(c.relation_parents(), [None, Some(0), Some(1)]);
    assert!(c
        .items()
        .iter()
        .all(|i| matches!(i.action, SourceAction::ProjectLocation { .. })));
    fs::write(root.join("d.p"), "/* modificado */").unwrap();
    assert!(c.activate_at_index(&mut app, Some(&root), 2, true).is_err());
    assert_eq!(app.active_path().unwrap().file_name().unwrap(), "a.p");
    fs::write(root.join("d.p"), &long_source).unwrap();
    c.activate_at_index(&mut app, Some(&root), 2, true).unwrap();
    assert_eq!(app.active_path().unwrap().file_name().unwrap(), "d.p");
    assert_eq!(app.active_bytes().unwrap(), long_source.as_bytes());
    drop(app);
    for entry in fs::read_dir(&root).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    fs::remove_dir(root).unwrap();
}

#[test]
fn abl_callers_keep_matches_if_a_project_file_exceeds_the_read_budget() {
    use std::fs;
    let fixture = std::env::temp_dir().join(format!("cide-nested-calls-{}", std::process::id()));
    fs::create_dir(&fixture).unwrap();
    let root = fixture.join("erp");
    fs::create_dir_all(root.join(".cide")).unwrap();
    fs::write(root.join(".cide/propath"), ".\n").unwrap();
    let target = root.join("destino.p");
    fs::write(&target, "/* destino */").unwrap();
    fs::write(root.join("origen.p"), "RUN destino.p.").unwrap();
    let large = fs::File::create(root.join("grande.p")).unwrap();
    large.set_len(64 * 1024 * 1024 + 1).unwrap();
    drop(large);
    let mut app = Workbench::default();
    app.open_file(&target, Some(TextEncoding::Utf8)).unwrap();
    let incoming = scan(&app, Some(&fixture), SourceMode::Incoming);
    assert_eq!(
        incoming.state(),
        SourceState::Incomplete,
        "{}",
        incoming.message()
    );
    assert!(incoming.message().starts_with("Análisis incompleto"));
    assert_eq!(incoming.items().len(), 1);
    incoming
        .activate_at_index(&mut app, Some(&fixture), 0, false)
        .unwrap();
    assert_eq!(app.active_path().unwrap().file_name().unwrap(), "origen.p");
    drop(app);
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn abl_calls_and_includes_are_separate_and_propath_selects_first_destination() {
    use std::fs;
    let root = std::env::temp_dir().join(format!("cide-source-propath-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for dir in [".cide", "primero", "segundo"] {
        fs::create_dir(root.join(dir)).unwrap();
    }
    let origin = root.join("origen.p");
    fs::write(&origin, "RUN destino.p.\n{datos.i}\nRUN VALUE(nombre).").unwrap();
    fs::write(root.join(".cide/propath"), "segundo\nprimero\n").unwrap();
    fs::write(root.join("primero/destino.p"), "/* primero */").unwrap();
    fs::write(root.join("segundo/destino.p"), "/* segundo */").unwrap();
    fs::write(root.join("primero/datos.i"), "/* include */").unwrap();
    let mut a = Workbench::default();
    let tab = a.open_file(&origin, Some(TextEncoding::Utf8)).unwrap();
    let includes = scan(&a, Some(&root), SourceMode::Dependencies);
    assert_eq!(includes.state(), SourceState::Ready);
    assert_eq!(includes.items().len(), 1);
    assert_eq!(
        includes.items()[0].action,
        SourceAction::RelativeFile("datos.i".into())
    );
    let mut calls = scan(&a, Some(&root), SourceMode::Calls);
    assert_eq!(calls.state(), SourceState::Ready);
    assert_eq!(calls.items().len(), 2);
    assert!(matches!(
        calls.items()[1].action,
        SourceAction::Unresolved(_)
    ));
    calls.activate(&mut a, Some(&root), true).unwrap();
    assert_eq!(a.active_bytes().unwrap(), b"/* segundo */");
    assert_eq!(
        a.panes()
            .view(jc_app::PaneId::Primary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    let mut callers = scan(&a, Some(&root), SourceMode::Incoming);
    assert_eq!(callers.state(), SourceState::Ready, "{}", callers.message());
    assert_eq!(callers.items().len(), 1);
    assert!(callers.items()[0].label.ends_with("origen.p:1"));
    fs::write(root.join(".cide/propath"), "primero\nsegundo\n").unwrap();
    assert!(callers.activate(&mut a, Some(&root), true).is_err());
    fs::write(root.join(".cide/propath"), "segundo\nprimero\n").unwrap();
    callers.activate(&mut a, Some(&root), true).unwrap();
    assert_eq!(
        a.active_path().unwrap().canonicalize().unwrap(),
        origin.canonicalize().unwrap()
    );
    a.open_file(root.join("primero/datos.i"), Some(TextEncoding::Utf8))
        .unwrap();
    let mut callers = scan(&a, Some(&root), SourceMode::Incoming);
    assert_eq!(callers.items().len(), 1);
    assert!(callers.items()[0].label.ends_with("origen.p:2"));
    callers.activate(&mut a, Some(&root), true).unwrap();
    assert_eq!(a.active_editor().unwrap().cursor, "RUN destino.p.\n".len());
    drop(a);
    for file in [
        "origen.p",
        ".cide/propath",
        "primero/destino.p",
        "segundo/destino.p",
        "primero/datos.i",
    ] {
        fs::remove_file(root.join(file)).unwrap();
    }
    for dir in [".cide", "primero", "segundo"] {
        fs::remove_dir(root.join(dir)).unwrap();
    }
    fs::remove_dir(root).unwrap();
}
#[test]
fn abl_calls_groups_local_names_before_the_result_limit() {
    let mut text = "RUN Fail.\n".repeat(100);
    text.push_str("RUN fAiL.\nRUN Otro.\nRUN otro.\nPROCEDURE Fail:\nEND PROCEDURE.\nPROCEDURE Otro:\nEND PROCEDURE.");
    let (mut app, mut calls) = run(&text, TextEncoding::Utf8, SourceMode::Calls);
    assert_eq!(calls.state(), SourceState::Ready, "{}", calls.message());
    assert_eq!(
        calls
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["RUN Fail", "RUN Otro"]
    );
    calls.activate(&mut app, None, false).unwrap();
    assert_eq!(
        app.active_editor().unwrap().cursor,
        text.find("PROCEDURE Fail").unwrap() + "PROCEDURE ".len()
    );

    let (_, refs) = run(
        "RUN Fail.\nRUN fail.\nPROCEDURE Fail:\nEND PROCEDURE.",
        TextEncoding::Utf8,
        SourceMode::References,
    );
    assert_eq!(refs.items().len(), 2);
}

#[test]
fn abl_call_groups_keep_distinct_targets_and_caller_routines() {
    use std::fs;
    let root = std::env::temp_dir().join(format!("cide-source-call-groups-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for dir in [".cide", "uno", "dos"] {
        fs::create_dir(root.join(dir)).unwrap();
    }
    let origin = root.join("origen.p");
    let text = format!(
        "PROCEDURE Primero:\nDO:\n{}END.\nRUN uno/destino.p.\nRUN dos/destino.p.\nEND PROCEDURE.\nFUNCTION Segundo RETURNS INTEGER():\nRUN uno/destino.p.\nRUN destino.p.\nRETURN 0.\nEND FUNCTION.\nRUN destino.p.\n",
        "RUN destino.p.\n".repeat(90),
    );
    fs::write(&origin, &text).unwrap();
    fs::write(root.join(".cide/propath"), "uno\n.\ndos\n").unwrap();
    fs::write(root.join("uno/destino.p"), "/* uno */").unwrap();
    fs::write(root.join("dos/destino.p"), "/* dos */").unwrap();

    let mut app = Workbench::default();
    app.open_file(&origin, Some(TextEncoding::Utf8)).unwrap();
    let mut calls = scan(&app, Some(&root), SourceMode::Calls);
    assert_eq!(calls.state(), SourceState::Ready, "{}", calls.message());
    assert_eq!(
        calls
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["RUN destino.p", "RUN dos/destino.p"]
    );
    calls.activate(&mut app, Some(&root), true).unwrap();
    assert_eq!(app.active_bytes().unwrap(), b"/* uno */");

    let mut incoming = scan(&app, Some(&root), SourceMode::Incoming);
    assert_eq!(
        incoming.state(),
        SourceState::Ready,
        "{}",
        incoming.message()
    );
    let labels = incoming
        .items()
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels.len(), 3, "{labels:?}");
    assert_eq!(labels[0], "Primero · origen.p:3");
    assert!(labels[1].starts_with("Segundo · origen.p:"));
    assert!(labels[2].starts_with("origen.p:"));
    incoming.activate(&mut app, Some(&root), true).unwrap();
    assert_eq!(
        app.active_editor().unwrap().cursor,
        text.find("RUN destino.p.").unwrap()
    );
    drop(app);
    for file in [
        "origen.p",
        ".cide/propath",
        "uno/destino.p",
        "dos/destino.p",
    ] {
        fs::remove_file(root.join(file)).unwrap();
    }
    for dir in [".cide", "uno", "dos"] {
        fs::remove_dir(root.join(dir)).unwrap();
    }
    fs::remove_dir(root).unwrap();
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
                "Procedimiento niño",
                "Función compute",
                "Clase Example",
                "Método work"
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
    assert_eq!(c.state(), SourceState::Ready);
    assert_eq!(c.items().len(), 65);
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
    assert_eq!(controller.items()[0].label, "Clase Example.Tools.Worker");
    let text=format!("FUNCTION {} RETURNS CustomType():\nEND FUNCTION.\nMETHOD PUBLIC SomeType {}():\nEND METHOD.","a".repeat(100),"b".repeat(100));
    let (_, controller) = run(&text, TextEncoding::Utf8, SourceMode::Symbols);
    assert_eq!(controller.state(), SourceState::Ready);
    assert_eq!(controller.items().len(), 2);
    assert_eq!(
        controller.items()[0].label,
        format!("Función {}", "a".repeat(100))
    );
    assert_eq!(
        controller.items()[1].label,
        format!("Método {}", "b".repeat(100))
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

#[test]
fn abl_outline_lists_declarations_and_static_includes_without_reading_targets() {
    let source = concat!(
        "{shared.i}\r\n",
        "{&not-a-file}\r\n",
        "/* DEFINE VARIABLE excluded AS INT. */\r\n",
        "PROCEDURE consume:\r\n",
        "  DEFINE INPUT-OUTPUT PARAMETER valor AS INTEGER.\r\n",
        "  DEFINE VARIABLE número AS INTEGER NO-UNDO.\r\n",
        "  DEFINE BUFFER cliente FOR customers.\r\n",
        "  DEFINE TEMP-TABLE tt-customer NO-UNDO FIELD id AS INT.\r\n",
        "  DEFINE BUTTON btn-save LABEL \"Guardar\".\r\n",
        "END PROCEDURE.\r\n",
    );
    let (mut app, controller) = run(source, TextEncoding::Windows1252, SourceMode::Symbols);
    assert_eq!(controller.state(), SourceState::Ready);
    let labels: Vec<_> = controller
        .items()
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Inclusión shared.i",
            "Procedimiento consume",
            "Parámetro valor",
            "Variable número",
            "Búfer cliente",
            "Tabla temporal tt-customer",
            "Botón btn-save",
        ]
    );
    let index = 3;
    controller
        .activate_at_index(&mut app, None, index, false)
        .unwrap();
    assert_eq!(
        app.active_editor().unwrap().cursor,
        controller.items()[index].range.start
    );
    app.insert_text("x").unwrap();
    assert!(controller
        .activate_at_index(&mut app, None, index, false)
        .is_err());
}

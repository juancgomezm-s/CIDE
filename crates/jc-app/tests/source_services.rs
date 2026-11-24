use jc_app::{
    source_services::{
        FileResolution, SourceAction, SourceController, SourceItem, SourceMode,
        SourceProviderDescriptor, SourceRegistry, SourceState,
    },
    Cancellation, JobSystem, TaskFailure, TaskItem, TaskOutput, TaskSnapshot, Wakeup, Workbench,
};
use jc_text::{TextEncoding, TextPosition};
use std::time::{Duration, Instant};
fn app(text: &str) -> Workbench {
    let mut a = Workbench::default();
    a.new_file("source-memory.txt", TextEncoding::Utf8);
    a.insert_text(text).unwrap();
    a
}

#[test]
fn complete_incoming_visits_every_file_past_browser_page_limits_and_keeps_versions() {
    use std::fs;
    let root = std::env::temp_dir().join(format!(
        "cide-complete-incoming-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let target = root.join("target.js");
    fs::write(&target, "export const valor=1;").unwrap();
    for index in 0..2100 {
        fs::write(
            root.join(format!("caller{index:04}.js")),
            "import './target.js';\n",
        )
        .unwrap();
    }
    let mut a = Workbench::default();
    a.open_file(&target, Some(TextEncoding::Utf8)).unwrap();
    let mut c = SourceController::outline_relationships(SourceMode::Incoming);
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(
        SourceMode::Incoming,
        &SourceRegistry::default(),
        &a,
        Some(&root),
        &mut jobs,
    )
    .unwrap();
    assert!(c.progress().is_some());
    let deadline = Instant::now() + Duration::from_secs(30);
    while jobs.outstanding() > 0 {
        for mut task in jobs.poll(&a, 4) {
            c.accept(&a, Some(&root), &mut task);
        }
        c.tick(&a, Some(&root), &mut jobs);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(c.state(), SourceState::Ready, "{}", c.message());
    assert_eq!(c.items().len(), 2100, "{}", c.message());
    assert!(c.message().contains("Nivel 1 completo"));
    assert!(c.progress().is_none());
    assert_eq!(c.items()[2099].label, "caller2099.js:1");
    fs::write(root.join("caller2099.js"), "// cambiado\n").unwrap();
    assert!(c
        .activate_at_index(&mut a, Some(&root), 2099, true)
        .is_err());
    drop(a);
    drop(jobs);
    for entry in fs::read_dir(&root).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    fs::remove_dir(root).unwrap();
}

#[test]
fn complete_outline_reads_past_two_mib_and_still_rejects_stale_generations() {
    let mut a = Workbench::default();
    a.new_file("grande.rs", TextEncoding::Utf8);
    a.insert_text(&format!(
        "/*{}*/\nfn final_visible() {{}}",
        "x".repeat(3 * 1024 * 1024)
    ))
    .unwrap();
    let mut c = SourceController::outline();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(
        SourceMode::Symbols,
        &SourceRegistry::default(),
        &a,
        None,
        &mut jobs,
    )
    .unwrap();
    assert!(jobs.snapshot_bytes() > 2 * 1024 * 1024);
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready, "{}", c.message());
    assert_eq!(c.items()[0].label, "final_visible · Función");
    c.request(
        SourceMode::Symbols,
        &SourceRegistry::default(),
        &a,
        None,
        &mut jobs,
    )
    .unwrap();
    a.insert_text(" ").unwrap();
    assert!(c.validate_context(&a, None, &mut jobs));
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Stale);
    assert!(c.items().is_empty());
}

#[test]
fn outgoing_tree_has_three_levels_in_each_code_language_and_keeps_navigation() {
    for (name, source) in [
        ("niveles.rs", "fn uno(){dos();} fn dos(){tres();} fn tres(){cuatro();} fn cuatro(){cinco();} fn cinco(){}"),
        ("niveles.js", "function uno(){dos();} function dos(){tres();} function tres(){cuatro();} function cuatro(){cinco();} function cinco(){}"),
        ("niveles.ts", "function uno(){dos();} function dos(){tres();} function tres(){cuatro();} function cuatro(){cinco();} function cinco(){}"),
        ("niveles.php", "<?php function uno(){dos();} function dos(){tres();} function tres(){cuatro();} function cuatro(){cinco();} function cinco(){}"),
        ("Niveles.java", "class Demo { void uno(){dos();} void dos(){tres();} void tres(){cuatro();} void cuatro(){cinco();} void cinco(){} }"),
        ("Niveles.cs", "class Demo { void uno(){dos();} void dos(){tres();} void tres(){cuatro();} void cuatro(){cinco();} void cinco(){} }"),
    ] {
        let mut a=Workbench::default(); a.new_file(name, TextEncoding::Utf8);a.insert_text(source).unwrap();
        let origin=source.find("uno").unwrap();a.set_linear_selection_bytes(origin,origin).unwrap();
        let mut c=SourceController::outline_relationships(SourceMode::Calls);let mut jobs=JobSystem::new(Default::default(),Wakeup::default()).unwrap();
        c.request(SourceMode::Calls,&SourceRegistry::default(),&a,None,&mut jobs).unwrap(); finish(&mut c,&a,&mut jobs);
        assert_eq!(c.state(),SourceState::Ready,"{name}: {}",c.message());
        assert_eq!(c.items().iter().map(|i|i.label.as_str()).collect::<Vec<_>>(),["dos","tres","cuatro"],"{name}");
        assert_eq!(c.relation_parents(),[None,Some(0),Some(1)]);
        assert!(c.present(10,80).rows[2].label.starts_with("    cuatro"));
        c.activate_at_index(&mut a,None,2,false).unwrap();assert_eq!(&source[a.active_editor().unwrap().cursor..][..6],"cuatro");
        assert!(a.navigation_back());assert_eq!(a.active_editor().unwrap().cursor,origin);
    }
}

#[test]
fn outgoing_recursion_is_visible_and_stops_without_looping() {
    let mut a = Workbench::default();
    a.new_file("recursivo.rs", TextEncoding::Utf8);
    let source = "fn uno(){dos();} fn dos(){uno();}";
    a.insert_text(source).unwrap();
    a.set_linear_selection_bytes(3, 3).unwrap();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(
        SourceMode::Calls,
        &SourceRegistry::default(),
        &a,
        None,
        &mut jobs,
    )
    .unwrap();
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.items().len(), 2);
    assert!(c.items()[1].label.ends_with("recursiva"));
    assert_eq!(c.relation_parents(), [None, Some(0)]);
}

#[test]
fn rust_outline_tracks_members_and_skips_non_code_in_an_unfinished_document() {
    let mut a = Workbench::default();
    a.new_file("esquema.rs", TextEncoding::Utf8);
    let text = r###"
//! struct Comentario;
macro_rules! generar { () => { fn MacroExpandida() {} } }
mod interno {
    #[doc = "struct Atributo;"]
    pub(crate) struct Conteo<T> { pub valor: T, r#type: usize }
    enum Estado { Inicio = 1 << 3, Lista { clave: String }, Otro }
    pub trait Resumen { fn obtener(&self); type Valor; }
    impl<T> Resumen for Conteo<T> where T: Copy {
        type Valor = T;
        fn obtener(&self) { let _ = r#"fn cadena() {}"#; fn interna() {} }
    }
    const fn versión() -> usize { 1 }
}
pub fn incompleta() {
"###;
    a.insert_text(text).unwrap();
    let registry = SourceRegistry::default();
    assert!(registry.supports_document(&a, SourceMode::Calls));
    let mut c = SourceController::outline();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(SourceMode::Symbols, &registry, &a, None, &mut jobs)
        .unwrap();
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    let items = c.items();
    for expected in [
        "generar · Macro",
        "interno · Módulo",
        "Conteo · Estructura",
        "valor · Campo",
        "r#type · Campo",
        "Inicio · Variante",
        "Lista · Variante",
        "clave · Campo",
        "Otro · Variante",
        "Resumen · Rasgo",
        "<T> Resumen for Conteo<T> · Implementación",
        "Valor · Tipo",
        "obtener · Método",
        "versión · Función",
        "incompleta · Función",
    ] {
        assert!(
            items.iter().any(|item| item.label == expected),
            "{expected}"
        );
    }
    for excluded in [
        "Comentario",
        "MacroExpandida",
        "Atributo",
        "cadena",
        "interna",
    ] {
        assert!(!items.iter().any(|item| item.label.starts_with(excluded)));
    }
    let implementation = items
        .iter()
        .find(|item| item.label.ends_with(" · Implementación"))
        .unwrap();
    let method = items
        .iter()
        .rfind(|item| item.label == "obtener · Método")
        .unwrap();
    assert!(
        matches!(&implementation.action, SourceAction::ScopedSymbol { extent } if extent.contains(&method.range.start))
    );
    let written = items
        .iter()
        .find(|item| item.label == "versión · Función")
        .unwrap();
    assert_eq!(
        &text.as_bytes()[written.range.clone()],
        "versión".as_bytes()
    );
}

#[test]
fn routine_relationships_follow_the_caret_in_each_code_provider() {
    for (name, source) in [
        (
            "demo.cs",
            r#"class Demo { void principal() { this.siguiente(); siguiente(); } void siguiente() {} } class Otro { void siguiente() {} }"#,
        ),
        (
            "Demo.java",
            r#"class Demo { void principal() { siguiente(); siguiente(); } void siguiente() {} } class Otro { void siguiente() {} }"#,
        ),
        (
            "demo.php",
            r#"<?php function principal() { SIGUIENTE(); siguiente(); } function siguiente() {} function otro() { externo(); }"#,
        ),
        (
            "miembros.php",
            r#"<?php class Demo { function principal() { $this->SIGUIENTE(); $this->siguiente(); } function siguiente() {} } class Otro { function siguiente() {} }"#,
        ),
        (
            "demo.rs",
            r#"fn principal() { siguiente(); siguiente(); } fn siguiente() {} fn otro() { externo(); }"#,
        ),
        (
            "demo.js",
            r#"function principal() { siguiente(); siguiente(); } const siguiente = () => {}; function otro() { externo(); }"#,
        ),
        (
            "demo.ts",
            r#"class Demo { principal(): void { this.siguiente(); this.siguiente(); } siguiente(): void {} } class Otro { siguiente(): void {} }"#,
        ),
    ] {
        let mut app = Workbench::default();
        app.new_file(name, TextEncoding::Utf8);
        app.insert_text(source).unwrap();
        let registry = SourceRegistry::default();
        let mut controller = SourceController::outline();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        let principal = source.find("principal").unwrap();
        app.set_linear_selection_bytes(principal, principal)
            .unwrap();
        controller
            .request(SourceMode::Calls, &registry, &app, None, &mut jobs)
            .unwrap();
        finish(&mut controller, &app, &mut jobs);
        assert_eq!(
            controller.state(),
            SourceState::Ready,
            "{name}: {}",
            controller.message()
        );
        let [target] = controller.items() else {
            panic!("{name}: {:?}", controller.items())
        };
        assert_eq!(target.label, "siguiente", "{name}");
        assert!(matches!(target.action, SourceAction::Navigate));
        assert!(controller.message().starts_with("principal ·"), "{name}");
        assert!(registry.supports_incoming(&app, None));
        let definition = target.range.start;
        controller.activate(&mut app, None, false).unwrap();
        assert_eq!(app.active_editor().unwrap().cursor, definition);
        assert!(controller.validate_context(&app, None, &mut jobs));
        controller
            .request(SourceMode::Incoming, &registry, &app, None, &mut jobs)
            .unwrap();
        finish(&mut controller, &app, &mut jobs);
        assert_eq!(
            controller.state(),
            SourceState::Ready,
            "{name}: {}",
            controller.message()
        );
        let [caller] = controller.items() else {
            panic!("{name}: {:?}", controller.items())
        };
        assert_eq!(caller.label, "principal", "{name}");
        assert!(
            source[caller.range.clone()].eq_ignore_ascii_case("siguiente"),
            "{name}: {:?}",
            app.syntax_rows(app.active_document().unwrap().tab_id, 0, 1)
        );
        assert!(caller.range.start < definition);
        let invocation = caller.range.start;
        app.set_linear_selection_bytes(invocation, invocation)
            .unwrap();
        assert!(
            app.code_link_at(app.active_document().unwrap().tab_id, invocation),
            "{name}: {:?}",
            app.syntax_rows(app.active_document().unwrap().tab_id, 0, 1)
        );
        controller
            .request(SourceMode::FollowLink, &registry, &app, None, &mut jobs)
            .unwrap();
        finish(&mut controller, &app, &mut jobs);
        assert_eq!(
            controller.items().len(),
            1,
            "{name}: {}",
            controller.message()
        );
        controller.activate(&mut app, None, false).unwrap();
        assert_eq!(app.active_editor().unwrap().cursor, definition, "{name}");
        assert!(app.navigation_back());
        assert_eq!(app.active_editor().unwrap().cursor, invocation, "{name}");
    }
}

#[test]
fn routine_relationships_do_not_resolve_qualified_or_ambiguous_calls() {
    let mut app = Workbench::default();
    let source = r#"function principal() { otro.duplicado(); duplicado(); privada(); /* ficticio() */ "oculto()"; }
function duplicado() {} function duplicado(x) {} function otra() { function privada() {} }"#;
    app.new_file("ambiguo.js", TextEncoding::Utf8);
    app.insert_text(source).unwrap();
    let at = source.find("principal").unwrap();
    app.set_linear_selection_bytes(at, at).unwrap();
    let mut controller = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    controller
        .request(
            SourceMode::Calls,
            &SourceRegistry::default(),
            &app,
            None,
            &mut jobs,
        )
        .unwrap();
    finish(&mut controller, &app, &mut jobs);
    assert_eq!(controller.items().len(), 3);
    assert!(controller
        .items()
        .iter()
        .all(|item| matches!(item.action, SourceAction::Unresolved(_))));
}

#[test]
fn method_mentions_and_callbacks_follow_definitions_in_every_code_language() {
    for (name, source) in [
        ("callback.rs", "fn principal() { let handler = siguiente; } fn siguiente() {}"),
        ("callback.cs", "class Demo { void principal() { Action handler = siguiente; } void siguiente() {} }"),
        ("Callback.java", "class Demo { void principal() { Runnable handler = this::siguiente; } void siguiente() {} }"),
        ("callback.php", "<?php class Demo { function principal() { $handler = self::siguiente(...); } function siguiente() {} }"),
        ("callback.js", "function principal() { usar(siguiente); } function siguiente() {}"),
        ("callback.ts", "function principal(): void { usar(siguiente); } function siguiente(): void {}"),
    ] {
        let mut app = Workbench::default();
        app.new_file(name, TextEncoding::Utf8);
        app.insert_text(source).unwrap();
        let cursor = source.find("siguiente").unwrap() + 2;
        let expected = source.rfind("siguiente").unwrap();
        app.set_linear_selection_bytes(cursor, cursor).unwrap();
        assert!(app.code_link_at(app.active_document().unwrap().tab_id, cursor), "{name}");
        let mut controller = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        controller.request(SourceMode::FollowLink, &SourceRegistry::default(), &app, None, &mut jobs).unwrap();
        finish(&mut controller, &app, &mut jobs);
        assert_eq!(controller.items().len(), 1, "{name}: {}", controller.message());
        controller.activate(&mut app, None, false).unwrap();
        assert_eq!(app.active_editor().unwrap().cursor, expected, "{name}");
        assert!(app.navigation_back());
        assert_eq!(app.active_editor().unwrap().cursor, cursor, "{name}");
    }
}

#[test]
fn markdown_references_select_link_at_caret_and_ignore_code_and_images() {
    let mut a = Workbench::default();
    a.new_file("enlaces.md", TextEncoding::Utf8);
    let text = "[Primero](uno.md)\n`[código](no.md)`\n![imagen](no.png)\n```md\n[falso](no.md)\n```\n[Segundo](guía%20rápida.md#inicio)\n[Web](https://example.com)";
    a.insert_text(text).unwrap();
    a.go_to_position(TextPosition { line: 6, column: 3 })
        .unwrap();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(
        SourceMode::References,
        &SourceRegistry::default(),
        &a,
        None,
        &mut jobs,
    )
    .unwrap();
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    assert_eq!(c.items().len(), 3);
    assert_eq!(c.selected(), 1);
    assert_eq!(
        c.items()[1].action,
        SourceAction::RelativeFile("guía rápida.md".into())
    );
    assert!(matches!(c.items()[2].action, SourceAction::Unresolved(_)));
    assert_eq!(a.active_bytes().unwrap(), text.as_bytes());
}

#[cfg(windows)]
#[test]
fn markdown_reference_opens_sibling_in_other_group_without_project() {
    use std::fs;
    let root = std::env::temp_dir().join(format!(
        "cide-document-links-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let source = root.join("indice.md");
    let target = root.join("mi guía.md");
    fs::write(&source, "[Guía](mi%20guía.md)").unwrap();
    fs::write(&target, "Contenido").unwrap();
    let mut a = Workbench::default();
    let tab = a
        .open_file(source.clone(), Some(TextEncoding::Utf8))
        .unwrap();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(
        SourceMode::References,
        &SourceRegistry::default(),
        &a,
        None,
        &mut jobs,
    )
    .unwrap();
    finish(&mut c, &a, &mut jobs);
    c.activate(&mut a, None, true).unwrap();
    assert_eq!(
        a.active_path().unwrap().canonicalize().unwrap(),
        target.canonicalize().unwrap()
    );
    assert_eq!(a.active_bytes().unwrap(), "Contenido".as_bytes());
    assert_eq!(
        a.panes()
            .view(jc_app::PaneId::Primary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    drop(a);
    fs::remove_file(source).unwrap();
    fs::remove_file(target).unwrap();
    fs::remove_dir(root).unwrap();
}
fn finish(controller: &mut SourceController, a: &Workbench, jobs: &mut JobSystem) {
    finish_in_project(controller, a, jobs, None);
}
fn finish_in_project(
    controller: &mut SourceController,
    a: &Workbench,
    jobs: &mut JobSystem,
    root: Option<&std::path::Path>,
) {
    let until = Instant::now() + Duration::from_secs(5);
    while jobs.outstanding() > 0
        || matches!(
            controller.state(),
            SourceState::Queued | SourceState::Running
        )
    {
        for mut task in jobs.poll(a, 4) {
            controller.accept(a, root, &mut task);
        }
        controller.tick(a, root, jobs);
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
}

#[test]
fn javascript_incoming_opens_static_import_at_its_line_in_other_group() {
    use std::fs;
    let root = std::env::temp_dir().join(format!("cide-js-incoming-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let source = root.join("caller.js");
    let target = root.join("target.js");
    let text = "// origen\r\nimport './target.js';\r\nexport * from './target.js';\r\nimport('./target.js');";
    fs::write(&source, text).unwrap();
    fs::write(&target, "export function demo() {}").unwrap();
    let mut a = Workbench::default();
    let tab = a.open_file(&target, Some(TextEncoding::Utf8)).unwrap();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(
        SourceMode::Incoming,
        &SourceRegistry::default(),
        &a,
        Some(&root),
        &mut jobs,
    )
    .unwrap();
    finish_in_project(&mut c, &a, &mut jobs, Some(&root));
    assert_eq!(c.state(), SourceState::Ready, "{}", c.message());
    assert_eq!(c.items().len(), 1, "{}", c.message());
    assert_eq!(c.items()[0].label, "caller.js:2");
    fs::write(&source, "import './other.js';").unwrap();
    assert!(c.activate(&mut a, Some(&root), true).is_err());
    assert!(!a.panes().is_split());
    fs::write(&source, text).unwrap();
    c.activate(&mut a, Some(&root), true).unwrap();
    assert_eq!(a.panes().active(), jc_app::PaneId::Secondary);
    assert_eq!(
        a.panes()
            .view(jc_app::PaneId::Primary)
            .unwrap()
            .location()
            .unwrap()
            .tab,
        tab
    );
    assert_eq!(a.active_path().unwrap(), source.canonicalize().unwrap());
    assert!(a.active_editor().unwrap().cursor >= "// origen\r\n".len());
    drop(a);
    fs::remove_file(source).unwrap();
    fs::remove_file(target).unwrap();
    fs::remove_dir(root).unwrap();
}
fn symbols(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    out.push(TaskItem::Source(SourceItem {
        value_type: None,
        range: 0..1,
        label: "one (lexical)".into(),
        action: SourceAction::Navigate,
    }))
}
fn registry() -> SourceRegistry {
    let mut r = SourceRegistry::default();
    r.register(SourceProviderDescriptor {
        id: "test.lexical",
        language: "text",
        symbols: Some(symbols),
        references: None,
        calls: None,
        callers: None,
        routines: None,
        dependencies: None,
        file_resolution: FileResolution::ProjectCandidates,
        completion: None,
        rename: None,
        snippets: &[],
    })
    .unwrap();
    r
}
#[test]
fn source_registry_capabilities_are_optional_namespaced_and_default_completion_needs_no_provider_process(
) {
    let mut r = registry();
    assert!(r.resolve("text", SourceMode::Symbols).is_some());
    assert_eq!(
        r.resolve("rust", SourceMode::Symbols).unwrap().id,
        "core.rust"
    );
    assert!(r.resolve("rust", SourceMode::Dependencies).is_none());
    assert_eq!(
        r.resolve("rust", SourceMode::Completion).unwrap().id,
        "core.words"
    );
    assert!(r
        .register(SourceProviderDescriptor {
            id: "bad id",
            language: "rust",
            symbols: Some(symbols),
            references: None,
            calls: None,
            callers: None,
            routines: None,
            dependencies: None,
            file_resolution: FileResolution::ProjectCandidates,
            completion: None,
            rename: None,
            snippets: &[],
        })
        .is_err());
    assert!(r
        .register(SourceProviderDescriptor {
            id: "test.lexical",
            language: "rust",
            symbols: Some(symbols),
            references: None,
            calls: None,
            callers: None,
            routines: None,
            dependencies: None,
            file_resolution: FileResolution::ProjectCandidates,
            completion: None,
            rename: None,
            snippets: &[],
        })
        .is_err());
    let a = Workbench::default();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    assert!(c
        .request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .is_err());
    assert_eq!(jobs.worker_count(), 0);
}
#[test]
fn source_local_word_completion_is_revision_checked_strict_and_one_undo() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let mut a = Workbench::default();
        a.new_file("words.txt", encoding);
        a.insert_text("niño cliente customer custom\r\nni").unwrap();
        let before = a.active_bytes().unwrap();
        let r = SourceRegistry::default();
        let mut c = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        c.request(SourceMode::Completion, &r, &a, None, &mut jobs)
            .unwrap();
        finish(&mut c, &a, &mut jobs);
        assert_eq!(c.state(), SourceState::Ready);
        assert_eq!(c.items().len(), 1);
        assert_eq!(c.items()[0].action, SourceAction::Replace("niño".into()));
        c.activate(&mut a, None, false).unwrap();
        assert!(a
            .active_bytes()
            .unwrap()
            .ends_with(&jc_text::encode("niño", encoding).unwrap()));
        a.undo().unwrap();
        assert_eq!(a.active_bytes().unwrap(), before);
    }
}
#[test]
fn source_completion_rejects_changed_caret_selection_revision_and_root() {
    for change in 0..4 {
        let mut a = app("customer custom cus");
        let r = SourceRegistry::default();
        let mut c = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        c.request(SourceMode::Completion, &r, &a, None, &mut jobs)
            .unwrap();
        finish(&mut c, &a, &mut jobs);
        match change {
            0 => a.move_left(false).unwrap(),
            1 => a.select_all().unwrap(),
            2 => a.insert_text("x").unwrap(),
            _ => {}
        };
        let root = (change == 3).then_some(std::path::Path::new("different-root"));
        let before = a.active_bytes().unwrap();
        assert!(!c.ready(&a, root));
        assert!(c.activate(&mut a, root, false).is_err());
        assert_eq!(a.active_bytes().unwrap(), before);
        assert!(c.validate_context(&a, root, &mut jobs));
        assert_eq!(c.state(), SourceState::Stale);
    }
}
#[test]
fn source_refresh_storm_cancel_and_bounded_presentation_do_not_retain_extra_snapshots() {
    let a = app("abcdef");
    let r = registry();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .unwrap();
    let charge = jobs.snapshot_bytes();
    for _ in 0..10 {
        c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
            .unwrap();
        assert_eq!(jobs.outstanding(), 1);
        assert_eq!(jobs.snapshot_bytes(), charge);
    }
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    for width in [0, 1, 20, usize::MAX] {
        let p = c.present(usize::MAX, width);
        assert!(p.rows.len() <= 64);
        assert!(p
            .rows
            .iter()
            .all(|r| r.label.is_ascii() && r.label.len() <= width.min(256)));
    }
    c.cancel(&mut jobs);
    assert!(c.items().is_empty());
    assert_eq!(c.state(), SourceState::Cancelled);
    assert_eq!(jobs.snapshot_bytes(), 0);
}
fn wrong_mode(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    out.push(TaskItem::Source(SourceItem {
        value_type: None,
        range: 0..1,
        label: "unexpected edit".into(),
        action: SourceAction::Replace("X".into()),
    }))
}
fn too_many(_: &TaskSnapshot, _: &Cancellation, out: &mut TaskOutput) -> Result<(), TaskFailure> {
    for _ in 0..65 {
        out.push(TaskItem::Source(SourceItem {
            value_type: None,
            range: 0..1,
            label: "symbol".into(),
            action: SourceAction::Navigate,
        }))?;
    }
    Ok(())
}
#[test]
fn source_provider_wrong_action_and_result_quota_cannot_publish_or_edit() {
    for (provider, expected) in [
        (wrong_mode as jc_app::TaskProvider, SourceState::Failed),
        (too_many, SourceState::Limited),
    ] {
        let mut r = SourceRegistry::default();
        r.register(SourceProviderDescriptor {
            id: "test.invalid",
            language: "text",
            symbols: Some(provider),
            references: None,
            calls: None,
            callers: None,
            routines: None,
            dependencies: None,
            file_resolution: FileResolution::ProjectCandidates,
            completion: None,
            rename: None,
            snippets: &[],
        })
        .unwrap();
        let a = app("abc");
        let mut c = SourceController::default();
        let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
        c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
            .unwrap();
        finish(&mut c, &a, &mut jobs);
        assert_eq!(c.state(), expected);
        assert!(c.items().is_empty());
        assert_eq!(a.active_bytes().unwrap(), b"abc");
    }
}
#[test]
fn source_snapshot_limit_is_checked_before_worker_start_and_completion_uses_only_prefix() {
    let mut a = app(&"x".repeat(2 * 1024 * 1024 + 8));
    let r = registry();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(Default::default(), Wakeup::default()).unwrap();
    assert!(c
        .request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .is_err());
    assert_eq!(c.state(), SourceState::Limited);
    assert_eq!(jobs.worker_count(), 0);
    a.go_to_position(TextPosition { line: 0, column: 8 })
        .unwrap();
    c.request(SourceMode::Completion, &r, &a, None, &mut jobs)
        .unwrap();
    assert_eq!(jobs.snapshot_bytes(), 8);
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    assert!(c.items().is_empty());
}

fn wait_cancel(
    _: &TaskSnapshot,
    cancel: &Cancellation,
    _: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    while !cancel.is_cancelled() {
        std::thread::yield_now();
    }
    Err(TaskFailure::Cancelled)
}
#[test]
fn source_temporary_shared_pressure_keeps_only_metadata_until_capacity_is_released() {
    let a = app(&"x".repeat(64));
    let r = registry();
    let mut c = SourceController::default();
    let mut jobs = JobSystem::new(
        jc_app::JobLimits {
            workers: 1,
            snapshot_bytes: 64,
            total_snapshot_bytes: 64,
            ..Default::default()
        },
        Wakeup::default(),
    )
    .unwrap();
    jobs.submit(
        &a,
        jc_app::JobKey(42),
        a.active_document().unwrap().id,
        None,
        0..60,
        wait_cancel,
    )
    .unwrap();
    c.request(SourceMode::Symbols, &r, &a, None, &mut jobs)
        .unwrap();
    assert_eq!(c.state(), SourceState::Queued);
    assert_eq!(jobs.snapshot_bytes(), 60);
    assert_eq!(jobs.outstanding(), 1);
    jobs.cancel(jc_app::JobKey(42));
    finish(&mut c, &a, &mut jobs);
    assert_eq!(c.state(), SourceState::Ready);
    assert_eq!(c.items().len(), 1);
    assert_eq!(jobs.snapshot_bytes(), 0);
}

use super::*;
use jc_app::{JobLimits, Wakeup};
use std::time::Instant;

fn complete(app: &mut Workbench, jobs: &mut JobSystem) -> CompletedTask {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(task) = jobs.poll(app, 1).pop() {
            app.accept_task_completion(&task);
            return task;
        }
        assert!(Instant::now() < deadline, "el AVM no entregó resultados");
        thread::sleep(Duration::from_millis(15));
    }
}

fn open(app: &mut Workbench, root: &Path, name: &str, content: &str) {
    let path = root.join(name);
    fs::write(&path, content).unwrap();
    app.open_file(path, None).unwrap();
}

#[test]
#[ignore = "requiere el proyecto sintético de OpenEdge con los ejemplos USING"]
fn progress_real_using_compiles_with_ordered_propath() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap())
        .canonicalize()
        .unwrap();
    let mut app = Workbench::default();
    let mut jobs = JobSystem::new(JobLimits::default(), Wakeup::default()).unwrap();
    for filename in [
        "consumidores/using-explicito.p",
        "consumidores/using-comodin.p",
        "primero/paquete/Derivada.cls",
    ] {
        let path = root.join(filename);
        let before = fs::read(&path).unwrap();
        app.open_file(&path, None).unwrap();
        app.reset_task_panels();
        submit(Action::Check, &app, &mut jobs, Some(&root)).unwrap();
        let result = complete(&mut app, &mut jobs);
        assert!(!completion(&result).1, "{filename}: {:?}", app.problems());
        assert!(app
            .output()
            .iter()
            .any(|row| row.contains("OpenEdge AVM 11.7")));
        assert!(app
            .output()
            .iter()
            .any(|row| row.contains("Sintaxis ABL comprobada")));
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
#[ignore = "requiere el proyecto sintético de esquema de OpenEdge con el ejemplo de asistencia"]
fn progress_real_schema_completion_compiles_with_propath() {
    let root = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap())
        .canonicalize()
        .unwrap();
    let mut app = Workbench::default();
    let mut jobs = JobSystem::new(JobLimits::default(), Wakeup::default()).unwrap();
    for filename in [
        "asistencia.p",
        "consumidores/consumidor.p",
        "consumidores/homonimo.p",
    ] {
        let path = root.join(filename);
        let before = fs::read(&path).unwrap();
        app.open_file(&path, None).unwrap();
        app.reset_task_panels();
        submit(Action::Check, &app, &mut jobs, Some(&root)).unwrap();
        let task = complete(&mut app, &mut jobs);
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(!completion(&task).1, "{:?}", app.problems());
        assert!(app
            .output()
            .iter()
            .any(|row| row.contains("OpenEdge AVM 11.7")));
        assert!(app
            .output()
            .iter()
            .any(|row| row.contains("Sintaxis ABL comprobada")));
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
#[ignore = "requiere CIDE_TEST_PROGRESS_EXE apuntando a un AVM local por lotes"]
fn progress_real_propath_compile_run_diagnostics_and_cancel() {
    let executable = std::env::var("CIDE_TEST_PROGRESS_EXE").unwrap();
    let fixture = JobDir::create().unwrap();
    let root = &fixture.path;
    for dir in [".cide", "primero con espacios", "segundo"] {
        fs::create_dir(root.join(dir)).unwrap();
    }
    let propath = root.join(".cide/propath");
    fs::write(&propath, "primero con espacios\nsegundo\n").unwrap();
    fs::write(root.join("primero con espacios/valor.i"), "{anidado.i}\n").unwrap();
    fs::write(
        root.join("primero con espacios/anidado.i"),
        "PUT UNFORMATTED \"Primero\" SKIP.\n",
    )
    .unwrap();
    fs::write(
        root.join("segundo/valor.i"),
        "PUT UNFORMATTED \"Segundo\" SKIP.\n",
    )
    .unwrap();
    let mut app = Workbench::default();
    let mut jobs = JobSystem::new(JobLimits::default(), Wakeup::default()).unwrap();
    open(&mut app, root, "programa.p", "{valor.i}\n");

    submit(Action::Check, &app, &mut jobs, Some(root)).unwrap();
    assert!(completion(&complete(&mut app, &mut jobs)).1);
    assert!(app.problems().iter().any(|r| r.contains(PROFILE)));
    app.reset_task_panels();
    fs::write(
        root.join(PROFILE),
        format!("executable={executable}\ncpinternal=iso8859-1\ncpstream=ibm850\n"),
    )
    .unwrap();
    submit(Action::Check, &app, &mut jobs, Some(root)).unwrap();
    assert!(
        !completion(&complete(&mut app, &mut jobs)).1,
        "{:?}",
        app.problems()
    );
    assert!(app
        .output()
        .iter()
        .any(|r| r.contains("Sintaxis ABL comprobada")));
    if let Ok(expected) = std::env::var("CIDE_TEST_PROGRESS_EXPECT_VERSION") {
        assert!(app
            .output()
            .iter()
            .any(|row| row.text == format!("OpenEdge AVM {expected}")));
    }
    assert!(!root.join("programa.r").exists());
    submit(Action::Compile, &app, &mut jobs, Some(root)).unwrap();
    assert!(
        !completion(&complete(&mut app, &mut jobs)).1,
        "{:?}",
        app.problems()
    );
    let artifact = app
        .output()
        .iter()
        .find_map(|row| row.strip_prefix("Código r generado en ").map(PathBuf::from))
        .expect("ruta de artefacto");
    assert_eq!(artifact.parent(), root.parent());
    assert!(artifact
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(&format!("cide-abl-{}-", std::process::id())));
    let artifact = JobDir {
        path: artifact,
        keep: false,
    };
    assert!(artifact.path.join("programa.r").is_file());

    for (order, expected) in [
        ("primero con espacios\nsegundo\n", "Primero"),
        ("segundo\nprimero con espacios\n", "Segundo"),
    ] {
        app.reset_task_panels();
        fs::write(&propath, order).unwrap();
        submit(Action::Run, &app, &mut jobs, Some(root)).unwrap();
        assert!(
            !completion(&complete(&mut app, &mut jobs)).1,
            "{:?}",
            app.problems()
        );
        assert!(
            app.output().iter().any(|r| r.text == expected),
            "{:?}",
            app.output()
        );
    }
    app.reset_task_panels();
    fs::write(&propath, ".\n").unwrap();
    submit(Action::Compile, &app, &mut jobs, Some(root)).unwrap();
    assert!(completion(&complete(&mut app, &mut jobs)).1);
    let row = app
        .problems()
        .iter()
        .find(|r| r.contains("valor.i"))
        .unwrap();
    assert!(row.target.is_some());
    app.activate_problem(row.id).unwrap();
    assert!(!app.output().iter().any(|r| r.contains("Código r generado")));

    app.reset_task_panels();
    open(
        &mut app,
        root,
        "ventana.w",
        "DEFINE VARIABLE cTexto AS CHARACTER NO-UNDO.\ncTexto = \"CIDE\".\n",
    );
    submit(Action::Check, &app, &mut jobs, Some(root)).unwrap();
    assert!(
        !completion(&complete(&mut app, &mut jobs)).1,
        "{:?}",
        app.problems()
    );
    assert!(!root.join("ventana.r").exists());

    app.reset_task_panels();
    open(
        &mut app,
        root,
        "error.p",
        "RETURN ERROR \"fallo de prueba\".\n",
    );
    submit(Action::Run, &app, &mut jobs, Some(root)).unwrap();
    assert!(completion(&complete(&mut app, &mut jobs)).1);
    assert!(
        app.problems().iter().any(|r| r.contains("fallo de prueba")),
        "{:?}",
        app.problems()
    );

    app.reset_task_panels();
    open(&mut app, root, "largo.p",
        "OUTPUT TO \"ejecutando.txt\".\nPUT UNFORMATTED \"inicio\".\nOUTPUT CLOSE.\nDO WHILE TRUE:\nEND.\n");
    submit(Action::Run, &app, &mut jobs, Some(root)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("ejecutando.txt").exists() {
        assert!(
            Instant::now() < deadline,
            "no se inició el procedimiento largo"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(submit(Action::Run, &app, &mut jobs, Some(root)).is_err());
    cancel(&mut jobs);
    assert_eq!(complete(&mut app, &mut jobs).status, TaskStatus::Cancelled);
    assert!(app.problems().is_empty());
    assert!(app.output().iter().any(|r| r.contains("cancelada")));
    assert_eq!(fs::read(root.join("programa.p")).unwrap(), b"{valor.i}\n");

    app.reset_task_panels();
    let profile_path = root.join(PROFILE);
    let profile = fs::read_to_string(&profile_path).unwrap();
    fs::write(&profile_path, format!("{profile}version=99.0\n")).unwrap();
    open(&mut app, root, "version.p",
        "OUTPUT TO \"version-ejecutada.txt\".\nPUT UNFORMATTED \"ejecutado\" SKIP.\nOUTPUT CLOSE.\n");
    submit(Action::Run, &app, &mut jobs, Some(root)).unwrap();
    assert!(completion(&complete(&mut app, &mut jobs)).1);
    assert!(app
        .problems()
        .iter()
        .any(|row| row.contains("requiere 99.0")));
    assert!(app
        .output()
        .iter()
        .any(|row| row.starts_with("OpenEdge AVM ")));
    assert!(!root.join("version-ejecutada.txt").exists());
    fs::write(&profile_path, profile).unwrap();
    submit(Action::Run, &app, &mut jobs, Some(root)).unwrap();
    assert!(!completion(&complete(&mut app, &mut jobs)).1);
    assert!(root.join("version-ejecutada.txt").is_file());
}

#[test]
#[ignore = "requiere el proyecto sintético de esquema de OpenEdge"]
fn progress_real_schema_with_aliases_and_failed_refresh_preserves_catalog() {
    let reference = PathBuf::from(std::env::var_os("CIDE_TEST_PROGRESS_SCHEMA_PROJECT").unwrap());
    let fixture = JobDir::create().unwrap();
    let root = &fixture.path;
    fs::create_dir(root.join(".cide")).unwrap();
    let profile = fs::read_to_string(reference.join(PROFILE)).unwrap();
    let pf = reference.join(".cide/desarrollo.local.pf");
    // The test owns the profile it modifies; it only reads the fixture databases.
    let profile = profile.replace(
        "pf=.cide/desarrollo.local.pf",
        &format!("pf={}", pf.display()),
    );
    fs::write(root.join(PROFILE), &profile).unwrap();
    let mut app = Workbench::default();
    let mut jobs = JobSystem::new(JobLimits::default(), Wakeup::default()).unwrap();
    let mut schema = SchemaController::default();
    let refresh = |schema: &mut SchemaController, app: &mut Workbench| {
        schema.request(Some(root), Wakeup::default()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(update) = schema.poll(Some(root), app) {
                return update;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(15));
        }
    };
    assert!(app.active_document().is_none());
    assert!(
        refresh(&mut schema, &mut app).first_output_row.is_some(),
        "{:?}",
        app.problems()
    );
    for expected in [
        "bases 2",
        "Base devlogica · aliases negocio",
        "Tabla devlogica.customer",
        "Base vacia · aliases archivo",
    ] {
        assert!(
            app.output().iter().any(|row| row.contains(expected)),
            "{expected}"
        );
    }
    assert!(!app.output().iter().any(|row| row.contains("Tabla vacia.")));
    open(&mut app, root, "alias.p",
        "FIND FIRST negocio.customer NO-LOCK NO-ERROR.\nIF AVAILABLE negocio.customer THEN PUT UNFORMATTED \"ALIAS-CIDE\" SKIP.\n");
    submit(Action::Run, &app, &mut jobs, Some(root)).unwrap();
    assert!(
        !completion(&complete(&mut app, &mut jobs)).1,
        "{:?}",
        app.problems()
    );
    assert!(app.output().iter().any(|r| r.text == "ALIAS-CIDE"));

    fs::write(
        root.join(PROFILE),
        profile.replace("negocio:devlogica", "negocio:noexiste"),
    )
    .unwrap();
    assert!(refresh(&mut schema, &mut app).problems);
    assert!(app.problems().iter().any(|row| row.contains("noexiste")));
    app.reset_task_panels();
    assert!(schema.show(&mut app).is_some());
    assert!(app
        .output()
        .iter()
        .any(|row| row.contains("Tabla devlogica.customer")));
    assert!(!app.output().iter().any(|row| row.contains("noexiste")));

    let profile_without_aliases = profile
        .lines()
        .filter(|line| !line.starts_with("aliases="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.join(PROFILE), profile_without_aliases).unwrap();
    app.reset_task_panels();
    assert!(
        refresh(&mut schema, &mut app).first_output_row.is_some(),
        "{:?}",
        app.problems()
    );
    assert!(app.output().iter().any(|row| row.text == "Base vacia"));

    schema.request(Some(root), Wakeup::default()).unwrap();
    schema.clear();
    let deadline = Instant::now() + Duration::from_secs(8);
    while schema.busy() {
        assert!(schema.poll(Some(&reference), &mut app).is_none());
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(15));
    }
    assert!(!schema.has_catalog());
}

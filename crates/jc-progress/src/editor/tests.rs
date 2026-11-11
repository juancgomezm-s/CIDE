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
}

# Validación

Estado de la primera base preparada antes de publicarla:

- Workspace Rust compilado localmente.
- Pruebas de edición por bytes, undo/redo, perfiles de lenguaje y protocolo Progress ejecutadas en el entorno de desarrollo.
- `cargo fmt`, `cargo test`, `cargo clippy -D warnings` y build release usados como comprobaciones de la base original.
- Se probaron bytes no UTF-8 y CRLF en la CLI headless.

## M1 — Archivo y texto interpretado

La corrida de cierre funcional previa al commit documental fue GitHub Actions **#30 / run 41269634082**. En Ubuntu y Windows completó con éxito:

- `cargo test --workspace --locked --offline`: **67 pruebas**, 0 fallos;
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`;
- build release del workspace;
- demo headless.

La matriz incluyó tests específicos de Windows para el ciclo de guardado que utiliza `ReplaceFileW`, además de creación no-clobber, fallo de escritura, conflicto externo, codecs legacy, UTF-8/BOM, line endings, byte↔posición y límites/compactación del historial.

La validación final del estado guardado se ejecuta nuevamente con `cargo fmt --check` estricto después de fijar el resultado de rustfmt.

## Política de validación desde M2

Las corridas de GitHub Actions registradas arriba son **evidencia histórica de M0/M1**, no la política de desarrollo para las épicas siguientes.

Por decisión D13, desde M2 hasta completar M4 no se ejecutará CI remoto automáticamente ni se utilizará GitHub Actions para corregir iterativamente errores introducidos mientras la arquitectura sigue cambiando. Las pruebas se ejecutan localmente y de forma dirigida cuando el entorno lo permite. Los huecos de validación se documentan y el hardening transversal queda concentrado en M5.

## M2A — Desacoplamiento multilenguaje

Se añadió a los 18 perfiles de `jc-languages` la capacidad declarativa de delimitadores de comentario y se retiraron del `jc-app` las tablas/casos específicos de nombres de lenguajes para comentar. Los tests de perfil se ampliaron para ABL, C#, XML, Python, JSON y PowerShell.

**Estado de validación:** cambios revisados estáticamente; compilación y pruebas Rust aún no ejecutadas en este entorno. GitHub Actions permanece deshabilitado para pushes y PRs hasta M5 por la decisión D13. No anunciar esta modificación como validada en Windows o Linux.

## M2A — Checkpoint doble panel / navegación

Se agregó código de `PaneManager` más integración inicial en `Workbench`: grupos Primary/Secondary, referencia anclable, back/forward independientes, scroll link como estado y compartición de `TabId`. Se añadieron seis pruebas de modelo y cuatro de integración (aún sin ejecución).

**Estado de validación:** cambios preparados y revisados estáticamente; **no se ha compilado, formateado ni probado** el checkpoint en esta sesión. Debe verificarse localmente en Windows con `cargo fmt --all -- --check`, `cargo test -p jc-app` y `cargo check -p jc-tui`. La UI de doble panel, navegación ABL por código, autocompletado, diff/merge y Git completo siguen pendientes. GitHub Actions no se utilizará antes de M5.

## Límites


No se ha ejecutado una AVM de Progress en este entorno. Los helpers ABL son experimentales hasta compilarlos/probarlos con las versiones objetivo.

No hay GUI todavía. No se ha medido memoria de una aplicación gráfica, tiempo de arranque, IME, accesibilidad ni rendimiento con archivos grandes.

Los números de rendimiento y los presupuestos del plan son objetivos de diseño, no resultados.

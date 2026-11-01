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

## Límites


No se ha ejecutado una AVM de Progress en este entorno. Los helpers ABL son experimentales hasta compilarlos/probarlos con las versiones objetivo.

No hay GUI todavía. No se ha medido memoria de una aplicación gráfica, tiempo de arranque, IME, accesibilidad ni rendimiento con archivos grandes.

Los números de rendimiento y los presupuestos del plan son objetivos de diseño, no resultados.

# Correspondencia entre editor y minimapa — 2026-11-24

Los enlaces de código conservan ahora el color de su token. El subrayado es
una propiedad independiente: un include ABL sigue siendo magenta aunque sea
navegable. `SyntaxRows::base_kind_at` expone esa clasificación sin cambiar el
contrato anterior de `kind_at`, utilizado por otros consumidores.

El minimapa residente conserva el estado del mismo analizador léxico del editor
desde el inicio del documento, en pasos de hasta 64 KiB. Así no interpreta como
código las líneas interiores de un comentario o una cadena multilínea. Mantiene
hasta 512 muestras de 192 bytes, admite y reconstruye hasta 32 filas por paso,
y conserva los datos sin recalcularlos cuando no cambia la revisión. Las claves
incluyen documento, revisión, pestaña, perfil y codificación; cada panel mantiene
su caché. Un límite léxico descarta los colores parciales conservadoramente.

La cobertura de los caracteres se obtiene de la fuente real del editor mediante
GDI, con una pequeña imagen de trabajo propia. El cambio de fuente, tamaño,
zoom o DPI invalida esa cobertura; cambiar el tema solo repinta los colores.
El color del texto ordinario también usa el tema del editor, sin sustituirlo
por el color de texto atenuado. No se cambia la fuente ni el contenido del
documento para dibujar el minimapa.

## Validación local en Windows

Toolchain y entorno del ejecutor `scripts/check-local.py`, caché debug
`target/embedded-git-next`, dos jobs y dependencias offline. Pasaron:

- `cargo test -p jc-app --lib workbench::syntax::tests --locked --offline`:
  6 pruebas, incluidos enlaces a clases e includes sin perder el tipo léxico.
- `cargo test -p jc-gui --lib minimap --locked --offline`: 7 pruebas. Cubren
  comentarios y cadenas multilínea, cambios de documento y revisión, retención
  de caché, dos archivos distintos en paneles separados, temas claro y oscuro,
  cobertura GDI distinta para Consolas y Courier New y navegación de archivo paginado.
- Formato y Clippy de `jc-gui`/`jc-app`, todos sus targets, sin advertencias.
- Compilación debug de `cide-gui`.
- `python scripts/smoke-native-gui.py --exe target/embedded-git-next/debug/cide-gui.exe --section syntax`:
  13 comprobaciones con ventana propia oculta y dibujo GDI real. Sin errores de
  pintura; fixture y metadatos Git intactos. No es una prueba de escritorio físico.

Registros en `work/minimap-fidelity/{tests,validation}.log`; evidencia nativa en
`work/native-gui-smoke/20261124T184707Z-716ba3a68c89`. Entrega independiente
`target/minimap-fidelity-next/debug`, con el runtime de OmniCore verificado.
Se preservan entregas anteriores y la ventana abierta por el usuario.

## Límites conservados

El minimapa es una reducción, no otro editor: retiene columnas limitadas y
muestras globales; Unicode no ASCII usa un glifo sustitutivo en la miniatura.
Los archivos paginados de varios GB mantienen su trabajador y muestras acotadas:
no se ha añadido un recorrido léxico completo de esos archivos y no se acredita
contexto multilínea global para ellos. No se repitió la suite completa, Linux,
TUI, MSI ni escritorio; no se publicaron cambios ni se ejecutó CI remota.

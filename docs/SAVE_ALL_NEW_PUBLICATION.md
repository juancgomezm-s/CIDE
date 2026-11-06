# Guardar todo: publicación de archivos nuevos — 2026-11-06

Se corrigió el fallo que detenía la prueba de Guardar todo antes de llegar a Git.
El enlace duro usado por `commit_new_path` devolvía WinError 5 en la ruta propia
del workspace. Un probe independiente reprodujo el rechazo sin abrir la GUI,
mientras que el movimiento sin reemplazo conservó los bytes y rechazó un destino
existente con WinError 183. No se atribuye el rechazo a OneDrive u otro proceso.

## Cambio

Solo en Windows, la publicación create-only usa `MoveFileExW` con WRITE_THROUGH,
sin REPLACE_EXISTING ni COPY_ALLOWED. El temporal sigue escribiéndose y cerrándose
después de flush/sync; se mueve en lugar de crear un enlace duro y borrarlo.
Si otro escritor crea el destino, la operación falla sin sobrescribirlo. No hay
fallback de copia, eliminación del destino ni reintentos de permisos.

Se convierten los padres existentes a rutas absolutas verbatim para conservar
las rutas largas; no se canonicaliza el archivo destino ausente. Se rechaza NUL.
La sesión solo adopta ruta/huella/revisión guardada tras el éxito. Undo y bytes,
incluidos BOM, CRLF y bytes inválidos, permanecen intactos. El reemplazo de archivos
existentes mantiene ReplaceFileW y su tratamiento previo de conflictos/contención.
El código de otras plataformas no cambia.

Contrato nativo: [MoveFileExW, Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw).
WRITE_THROUGH no se presenta como una garantía universal ante cortes de energía.

## Validación dirigida, sin escritorio

- Tres regresiones nuevas de jc-files: guardado nuevo exacto con deshacer,
  aparición del destino durante escritura del temporal y ruta Unicode de más
  de 260 unidades. Pasaron las tres; no se repitió el resto de la suite.
- Un test nuevo del parser para la sección aislada de guardado, con rutas
  obligatorias, duplicados y exclusividad. Pasó; demás tests filtrados.
- `--smoke-save-only` / `--section save` ejecuta únicamente los tres escenarios
  existentes de Save All: cancelación que conserva pendientes, asignación explícita
  con vistas/undo intactos y conflicto externo sin sobrescritura. Los tres pasaron,
  más los ocho controles comunes de arranque/DPI/pintura del fixture oculto.
- Formato y Clippy all-targets de jc-files/jc-gui aprobaron offline. Se compila
  release en `target/save-all-next/release`; se preservan las rutas anteriores.

El HWND permaneció oculto, sin ratón, teclado físico, portapapeles personal ni
recuperación del usuario. El archivo inicial y los metadatos Git auxiliares del
fixture quedaron intactos. No se repitieron pruebas Git, escritorio, suites
completas ni CI. El primer intento de compilar la regresión usó unwrap sobre
el bool de undo; se corrigió a una aserción y se conserva el log del fallo.

Logs: `work/save-new-before.json`, `work/save-all-files-tests-close.log`,
`work/save-all-parser-tests.log`, `work/save-all-native.log`,
`work/save-all-fmt-check.log`, `work/save-all-clippy.log`, `work/save-all-release.log`.
Evidencia estructurada: `docs/evidence/save-all-new-publication-2026-11-06.json`.
La suite HWND completa queda sin repetir, no se declara una aprobación global M2.

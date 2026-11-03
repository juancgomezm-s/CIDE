# Selección literal y ciclo de pestañas GUI — 2026-11-03

Bloque posterior a `45fe0d1`. Integra SearchSelectionNext/Previous y
FileReopenClosed, más las rutas pendientes de SearchCancel y Tab1…9 ya existentes.
Menús, atajos y paleta usan registro/Workbench; no hay otro matcher, buffer o undo.

## Uso y selección

Ctrl+F3 / Ctrl+Shift+F3 buscan la selección siguiente/anterior; también Search y
paleta. Sin selección lineal en Editor, con rectángulo o foco modal/Explorer se
deshabilitan: no inventan la palabra del caret ni buscan la selección oculta de
otra vista. La paleta revalida documento/revisión/encoding/ViewId/pane/caret/anchor.

La selección se interpreta con su encoding explícito, sin conversión del fuente.
Hasta4096 bytes del rango fuente y hasta4096 bytes UTF-8 de consulta; una expansión
legacy que exceda el segundo límite se rechaza antes de cambiar la consulta.
Secuencias inválidas se rechazan, sin fabricar U+FFFD. Se preservan flags de case/
whole-word y EOL exactos; literal significa sin regex. Usa el scanner S0 cooperativo,
su historial20, presupuestos, generaciones y descarte de navegación F3 obsoleta.
Una consulta vigente ya Complete puede navegar inmediatamente; una consulta nueva
se procesa por turnos. Las pruebas de entrega tardía fuerzan Running antes de editar.
Search / Cancel Document Search cancela ese mismo trabajo y/o selección rectangular;
no modifica bytes ni guarda. No se consume trabajo durante prepare/draw.

## Cierre y reapertura

Ctrl+Shift+T, File / Reopen Closed Tab y paleta consumen el último cierre. Las32
entradas contienen metadatos mínimos (tab/path/encoding/new-file), no contenidos,
carets recuperables o undo. Un cierre compartido elimina solo una vista y no crea
entrada cerrada. La última vista dirty exige confirmar descarte o guardar antes.
Cancelar no crea una entrada. La confirmación nativa suelta préstamos y revalida
el destino completo al volver; una revisión/vista/intención distinta exige reintentar.

Una reapertura genuina reutiliza TabId y crea **DocumentId y undo nuevos** desde
el disco actual. Una entrada que era new-file se recrea vacía. No recupera cambios
descartados ni ofrece undo global del cierre. La codificación retenida se mantiene;
si es ambigua se abre la elección existente. El cursor inicia tras BOM cuando
corresponde. Se activa explícitamente en el grupo actual, incluso si está fijado;
el pin protege navegación automática, no la selección explícita de pestaña.

`try_reopen_closed` conserva ambos stacks y el foco/vistas si falla la lectura,
informando el error. `reopen_closed` es su wrapper bool de compatibilidad. Una
ruta exacta/canónica ya abierta se rechaza explícitamente, conservando el documento
dirty existente y la entrada cerrada; no se crea otro buffer ni se recarga ese
documento. El historial es LIFO: una entrada fallida permanece para reintento,
sin saltar silenciosamente a una más antigua. Se valida la identidad de ambos
stacks antes de leer/admitir. No es un snapshot atómico del filesystem.

Alt+1…8 elige la posición del catálogo global y Alt+9 la última; ahora también en
Navigation y paleta. Los comandos conservan las vistas persistentes y su undo;
una posición inexistente no se ajusta ni crea pestaña. No se prometen tabs por
grupo ni restauración de selección de la sesión ya destruida al cerrar la última vista.

## Validación del bloque

Se construyó y cableó todo antes de checks. Un worker existente produjo los dos
archivos exclusivos de integración GUI/TUI; un integrador para API/State, rutas,
menús, scripts, validación y Git. Rust1.85.1 MSVC x64, local/offline:

- **11 casos dirigidos distintos aprobados:**dos core de selección/reapertura,
  cuatro regresiones TUI nuevas y cinco parser CLI GUI. No corrida global.
- fmt, Clippy workspace/all-targets -D warnings y release workspace bins/examples
  aprobados. Sin dependencias nuevas ni otro motor de búsqueda/edición.
- **15 checks nativos aprobados**, ocho nuevos y siete arranque/pintura, sección
  `selection-reopen` de HWND oculto propio. Un WM_PAINT, cero errores; no suites
  cerradas de líneas/comentarios, navegación o Explorer repetidas.
- Selección Unicode/multilínea en cuatro encodings, flags, cuota4096 raw válida,
  raw4098/expansión legacy a8192UTF8 inválidas, bytes inválidos, vacío/rectángulo,
  foco modal/Explorer/paleta obsoleta, cancel y entrega tras cambio de vista/revisión.
- Disco actual, mismoTabId/nuevoDocumentId, LIFO, archivo ausente/reintento,
  colisión con documento dirty abierto, cierre de una vista compartida, cancelar/
  confirmar último dirty y confirmación obsoleta. New-file recrea vacío; no hay
  recuperación de borradores o undo destruido. Tabs1…8 y último por menú/paleta/
  Alt conservan identidad, caret/selección y flags.

Primera corrida TUI:3 aprobaron y una asumió entrega diferida aun con consulta ya
Complete; se corrigió el montaje para exigir Running/selección original, y solo
esa prueba se repitió. Compilación GUI señaló un getter privado usado por el
arnés: se sustituyó por vistas públicas y transporte de decisión sin duplicar la
cuenta privada. No se amplió API solo para tests. Se retienen ambos fallos crudos.

[Logs y fuentes/binarios SHA-256](evidence/m2b-selection-reopen-2026-11-03-metadata.json),
[reporte15](evidence/m2b-selection-reopen-2026-11-03-native-report.json),
[selección encontrada](evidence/m2b-selection-reopen-2026-11-03-native-selection-search.png),
[fuente original](evidence/m2b-selection-reopen-2026-11-03-native-frame.png).
PNG1000×650 GDI→PNG sin pérdida, revisados visualmente. Fuente13B y metadataGit
sintética intactas; no otra validación de proveedores Git. Seis ayudas/aliases y
siete frames por PE aprobados. Fixtures propios create_new y limpieza exacta.

ZIP local de desarrollo `work/packages/CIDE-windows-dev-selection-reopen-2026-11-03.zip`:
3 934 566 bytes,69 miembros, CRC y hashes del manifiesto aprobados.
[SHA-256 y notices](evidence/m2b-selection-reopen-2026-11-03-package.json).
No instalador ni certificación de distribución; los dos notices GNU siguen pendientes.

No pantalla física, clipboard OS, IME/DPI/accesibilidad, Linux, RAM global o AVM
certificados. M2A/M2B siguen abiertos; symlink omitido y notices GNU del ZIP siguen
pendientes. Sin CI, push, publicación, instalaciones o perfiles cerrados repetidos;
las pruebas manuales del propietario continúan al final.

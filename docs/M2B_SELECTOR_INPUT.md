# Entrada y clipboard de selectores GUI — 2026-11-03

Bloque posterior a `1b2b539`, dentro del hueco de entrada avanzada ya registrado
en el inventario. Quick Open, Command Palette y selección de encoding comparten
edición de campo, selección, entrada por teclado, Cut/Copy/Paste y presentación.
No añade proveedor, dependencia, control EDIT, buffer fuente ni historial nuevo.

## Contrato de interacción

| Entrada | Resultado |
|---|---|
| Left/Right/Home/End | Mover caret del campo; Left/Right colapsan selección a su borde |
| Shift + esas teclas | Extender selección del campo |
| Backspace/Delete | Borrar selección o escalar contiguo |
| Ctrl+A / Ctrl+U | Seleccionar todo / limpiar campo |
| Ctrl+C/X/V y Edit Copy/Cut/Paste/Select All | Operar el campo activo, conservando el selector |
| Up/Down/PgUp/PgDn, Tab/Shift+Tab | Recorrer resultados, sin insertar sangría en fuente |
| Ctrl+Home/End | Primer/último resultado; Home/End sin Ctrl ya pertenece al campo |
| F5 en Quick Open | Refresh explícito de la consulta vigente |
| Enter / Ctrl+Enter | Aceptar; en Files Ctrl+Enter abre al lado según política existente |
| Escape | Cancelar y restituir contexto modal/invocador conservado |

Las posiciones del campo son bytes UTF8 con fronteras escalares, no grafemas ni
columnas de fuente. El editor compartido mantiene CRLF indivisible para campos
multilínea; los selectores continúan siendo de una línea lógica y **256 bytes UTF8**.
Controles, CR/LF/TAB/NUL y marcadores bidi listados por la política existente se
rechazan completos. El cálculo de cuota descuenta primero la selección reemplazada;
si falla, texto, caret, anchor, consulta/resultados e intención no cambian. No se
normaliza Unicode ni se transcodifica fuente para escribir una consulta.

Editar texto reinicia la selección de candidato y solicita la última consulta
Files al controlador existente. Mover caret/anchor no relanza búsquedas ni cambia
el candidato. Solo los mensajes de tareas aceptan resultados versionados. Una
respuesta antigua no puede publicar candidatos para otra raíz/consulta/generación.

## Propiedad, foco y comandos

`FieldSelection` y `edit_selected_field` reutilizan las fronteras y operaciones
de `edit_field` en jc-app. Selección de texto y ordinal de candidato son estados
distintos; el campo posee un String y su caret/anchor una sola vez. No participa
en undo/redo del documento. El motor de edición de fuentes sigue en Workbench.

`CommandContext.field_input` permite clipboard/SelectAll de controles de entrada;
no autoriza sangría, comentarios, undo ni otras mutaciones de fuente. La GUI lo
activa solo para la **entrada de menú/tecla** de esos cuatro comandos mientras
hay selector. La disponibilidad/aceptación de comandos **listados en la paleta**
conserva el contexto del invocador. Elegir `Copy` en la lista y aceptar primero
cierra la paleta y ejecuta la intención original; Ctrl+C dentro del campo copia
su selección. No se confunden ambas rutas para habilitar edición oculta.

Escape conserva Find/Replace movido temporalmente a la paleta y restaura Explorer
invocador en los tres selectores. El foco inferior subyacente también se conserva.
Files/Encoding aceptados exitosamente vuelven al editor. Vista, pin, sesiones dirty,
caret/selección de fuente y undo siguen las reglas de los controladores existentes.

## Handoff nativo y privacidad

Cada operación captura generación del selector (no se reutiliza al cerrar/reabrir),
revisión de intención del campo, candidato, kind, raíz, ruta y destino de documento/revisión/encoding/
ViewId/pane/caret/anchor/rectángulo, invocador, foco y target de encoding pertinente.
Mover caret o selección también invalida una intención de clipboard pendiente.
Una generación agotada falla explícitamente en lugar de volver a usar IDs.

El adaptador nativo opera solo por acción explícita y **sin préstamo RefCell/modelo**:
delayed rendering puede reentrar y procesar mensajes. Antes y después de la llamada
se compara el destino. Cut elimina la selección únicamente tras publicación y
revalidación; Paste inserta únicamente tras lectura estricta/revalidación y preflight.
Copy/Cut sin selección no publican; texto vacío o formato ausente no alteran campo.
Un fallo de publicación o destino obsoleto no borra ni pega parcialmente. Como en
el adaptador previo, un clipboard ya publicado no tiene rollback global si luego
cambia el destino o falla el cierre nativo; esa limitación no se oculta.

Para selectores, la salida UTF8 del decoder nativo se preflightea a256 bytes antes
de reservarla. Se conserva el límite previo de2MiB de la asignación UTF16 nativa
incluido padding, validación estricta de surrogates/NUL/trailing data y cero polling.
Este límite de asignación ajena no es RAM total ni deadline del delayed rendering.
La inserción vuelve a comprobar cuota posterior al reemplazo y política de campo.

Durante construcción y automatización se usa únicamente texto sintético controlado
y `smoke_clipboard`/transport inyectado existente. Las pruebas no abren, leen ni
modifican el clipboard privado del usuario. No se registran contenidos ajenos;
interoperabilidad clipboard OS real sigue pendiente al final con el usuario.

## Presentación y límites

`present_field` y `present_selected_field` comparten un único recorrido de escapes,
CRLF y wrap. Dos pasadas acotadas localizan caret/ventana y retienen solo hasta32
filas visibles, evitando materializar todas las filas escapadas de un campo4096B
con ancho1. La nueva ruta no decodifica documentos fuente; el costo de dos pasadas
se limita a campos pequeños. No equivale a medición de latencia física o RAM privada.

El selector conserva un snapshot propio pequeño de consulta/intención; GDI compone
escapes ASCII, caret y rangos seleccionados del mismo mapa. Si un escape se envuelve,
sus fragmentos visibles conservan selección. Una fila de consulta sigue al caret
con viewport acotado; resultados permanecen virtualizados según el layout previo.
La composición no cambia foco, input, resultados, documento ni admisión de trabajos.
No se promete IME/grafemas, colocación del caret por mouse, drag lineal/wheel o
adaptación completa de aceleradores de menú; esos huecos son independientes.

## Validación local

Se registra al cerrar el bloque integrado, distinguiendo productores/transporte
sintéticos de interoperabilidad OS y evidencia histórica de ejecutada ahora.

Construcción completa antes de checks, con worker existente en text_field.rs y
después selector_input_smoke.rs, archivos exclusivos; integrador en exports/API,
State/rutas/transport/pintura/manifest y evidencia. **23 casos Rust distintos
aprobados**: cinco de campo compartido, siete selector/modelo/pintura, cuatro
decoder, seis TUI multilínea afectados y un parser CLI. Son24 pases porque el
filtro selector incluyó también el caso nuevo del decoder, luego repetido dentro
de su módulo. La metadata deduplica los nombres; no se suman como24 distintos.
Cuatro pruebas Rust nuevas; no suite global ni módulos cerrados sin impacto.

**13 checks HWND aprobados**, seis nuevos y siete de arranque/pintura. Tres clases
de selector con edición/clipboard sintético por tecla/menú, Unicode exacto, reemplazo
al límite/rechazos atómicos/empty y navegación de candidatos. Quick Open usa
metadata/Wakeup/TASKS_MESSAGE reales, supersession, buffer dirty reutilizado y
apertura lateral/pin/undo sobre fixture propio. Nueve reentradas sintéticas de
transporte cambian caret/query/kind/revisión/vista/encoding/raíz/ABA/candidato;
operación tardía rechazada sin cut/paste parcial. Foco Find/Explorer/Bottom,
aceptación de encoding sin guardar/transcodificar, resize real/compact, composición
sin mutación/consumo/admisión y limpieza. Source13B, encoding, DocumentId/revisión,
catálogo y metadata Git sintética conservados; no se crearon archivos fuente nuevos.
El ciclo dirty/undo de Quick Open es una acción explícita del fixture; los campos
por sí mismos no alteran su historial fuente. Un WM_PAINT, cero errores.

fmt, Clippy workspace/all-targets `-D warnings` y release workspace bins/examples
aprobados, Rust1.85.1 MSVC offline. Se conservan fallos iniciales: expectativa del
fixture oversize sin escapes `\s`, dos compilaciones del arnés por invocar un
getter privado y Clippy por payload grande de Action. Expectativa corregida y
solo caso fallido repetido; arnés usa vistas públicas; el handoff lleva Box para
no agrandar todas las acciones a320 bytes. Esa asignación/metadata acotada existe
solo por clipboard explícito, sin dependencia nueva. Native pasó a la primera.

[Hashes/logs y nombres deduplicados](evidence/m2b-selector-input-2026-11-03-metadata.json),
[reporte nativo](evidence/m2b-selector-input-2026-11-03-native-report.json),
[campo seleccionado](evidence/m2b-selector-input-2026-11-03-native-selector-input.png),
[fuente restaurada](evidence/m2b-selector-input-2026-11-03-native-frame.png).
PNG1384×741 y984×641 revisados visualmente tras conversión BMP GDI→PNG sin pérdida.
No certifican clipboard OS/IME/DPI/accesibilidad, mouse/pantalla físicos, Unicode
general, Linux ni RAM global. Selección lineal por drag/wheel y adaptación de menús
siguen pendientes; también symlink opcional y dos notices GNU del ZIP. Sin nuevos
proveedores/AVM/regex/dependencias/instalaciones, CI/push/publicación o seguridad.

Paquete local `work/packages/CIDE-windows-dev-selector-input-2026-11-03.zip`:
4.095.956 bytes, SHA-256 `2327701365ff5c112a8e1b02318402c5e64801073daf77316915a718e830eb40`.
CRC y hashes de69 entradas del manifiesto verificados; [resultado](evidence/m2b-selector-input-2026-11-03-package.json).
Dos notices GNU pendientes, sin certificación de distribución/instalador.

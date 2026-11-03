# S0 — Búsqueda cooperativa y Unicode

Incremento del 2026-11-02 sobre S1 `6e07b1e`, rama `m2a/dual-pane-tui`.
Se retiró el escaneo síncrono/comparación ASCII del documento y el KMP de bytes
separado de proyecto. Ambos consumidores usan un único matcher literal Unicode
por fragmentos en `jc-app/text_search.rs`. S1 conserva su interfaz de coincidencia
exacta sensible a mayúsculas; S0 admite case folding completo y palabra completa.

## Flujos implementados

- Ctrl+F actualiza la consulta incrementalmente. Enter/F3 y Shift+Enter/Shift+F3
  navegan, también dentro del diálogo. Una navegación solicitada antes de tener
  resultados espera al turno que encuentre el destino o confirme el wrap al EOF.
- Ctrl+H permite sustituir el actual o todos (Ctrl+Enter), con revisión/encoding
  comprobados y una transacción. Las búsquedas parciales/canceladas/limitadas no
  permiten reemplazar como si fueran completas. El reemplazo no guarda el archivo.
- Ctrl+F3 / Ctrl+Shift+F3 buscan la selección siguiente/anterior. No inventan una
  palabra cuando no hay selección; el comando se deshabilita en ese contexto.
- Ctrl+G acepta `línea[:columna]`, una-based de escalares. Cero, overflow y posiciones
  fuera de línea/columna se rechazan sin mover la vista ni entrar dentro de CRLF.
- Alt+C y Alt+W en Find/Replace cambian case/palabra completa. Alt+Up/Alt+Down
  recuperan consultas aceptadas con sus flags. Historial de memoria: 20 entradas.
- Las coincidencias visibles tienen marcadores de estilo en ambos grupos que
  muestran el mismo documento. Son resultados de la consulta, no bookmarks
  persistentes. El renderer no avanza búsquedas ni modifica caches textuales.
- Esc en Find/Replace cancela trabajo pendiente y cierra el diálogo. Esc en Editor
  o Search / Cancel Document Search cancelan un scan activo. La cancelación se
  conserva al editar; reabrir Find/Replace, cambiar consulta/flags o límites reinicia.

Regex permanece deshabilitado: no se añadió un motor ni se ofrece un checkbox
funcional aparente. Su API devuelve error sin alterar opciones/resultados. El
producto aún no acredita cierre completo de Search Plus/M2A ni paridad de GUI.

## Propiedad y presupuesto

`set_search_query` solo programa. No copia ni decodifica el documento. Workbench
posee una tarea cooperativa sobre rangos de su documento actual; los préstamos
de lectura duran un turno y se validan por DocumentId/revisión/encoding. No se
añade un worker, snapshot completo ni segundo buffer editable.

`poll_document_search` procesa un fragmento o la parte restante del anterior y
devuelve el control al teclado. El loop comprueba entrada entre turnos y solo
redibuja al cambiar resultados, selección o estado. No necesita otra tecla para
terminar una consulta; al no quedar trabajo vuelve a bloquear. Cambios de fuente,
encoding o documento invalidan inmediatamente los resultados y reprograman una
consulta activa; generaciones canceladas no reaparecen por editar.

Cuotas predeterminadas:

| Recurso | Cota |
|---|---|
| Consulta / reemplazo | 4096 bytes UTF-8 |
| Turno | Un fragmento de hasta 16 KiB, con hasta 4 bytes de contexto UTF-8 |
| Tiempo por turno | Objetivo/presupuesto cooperativo 2 ms, consultado cada 128 escalares |
| Entrega | Hasta 256 coincidencias por turno |
| Consulta completa | 16 MiB examinados, 10.000 coincidencias, 5 s de deadline |
| Configuración máxima | Fragmento 64 KiB, scan 64 MiB, 10.000 resultados, turno 10 ms y deadline 30 s |
| Historial | 20 consultas de hasta 4096 bytes y flags |
| Replace-all | 2 MiB de propuestas textuales, antes de publicar el lote |
| Go to | Hasta 64 bytes de entrada, sin valores cero/overflow |

En este checkpoint el decoder conservaba un fragmento, contexto, incidencias y offsets;
el bloque posterior S0_DECODER_OPTIMIZATION retira esa materialización del scan; el matcher
guarda un mapa de origen para el prefijo que está coincidiendo. No se escribe un
Origin por cada escalar ajeno a la consulta. Resultados tipados contienen rango
de bytes y posición escalar; no se conserva otra lista espejo de posiciones.
Las capacidades y datos generados son acotados, pero estas cuotas no son un límite
global de RAM privada. Decodificar un fragmento y una llamada al allocator no son
preemptibles: el presupuesto temporal no es una garantía dura del planificador.

Los estados distinguen Idle/Running/Complete/Limited/Cancelled/TimedOut/Unavailable.
La cuota no trunca silenciosamente un replace-all; exige refinar o reiniciar.
El guardado Windows conserva `ed51143`, incluidos recovery y errores persistentes.

## Unicode y bytes originales

Datos oficiales Unicode **17.0.0**, fijados por SHA-256 y generados offline mediante
`scripts/generate-unicode-search.py`. No se añadió crate ni descarga en build/runtime.
Ver [procedencia/licencia](../third_party/unicode-17/README.md). CaseFolding.txt se
conserva como oráculo independiente para comprobar las 1585 entradas C/F.

La comparación insensible utiliza **default full case folding C+F, no Turkic**.
Ejemplos: `Straße`/`STRASSE`, Σ/ς/σ y las expansiones de İ o ﬃ. No es lowercase
ASCII, ni lowercase escalar simple. No aplica NFC/NFD, no elimina acentos y no
reescribe fuente, BOM, encoding, EOL ni newline final.

Cada escalar plegado mantiene su origen. Un match debe empezar/terminar en un
escalar original completo: `ss` puede sustituir ß, pero `s` no sustituye media ß.
Tampoco entra en CRLF. Las secuencias inválidas son barreras de búsqueda, no texto
canónico U+FFFD inventado; una U+FFFD válida sí es buscable. El contexto adicional
del decoder mantiene grupos UTF-8 aunque un fragmento termine a mitad de secuencia.

La regla de palabra es UCD L/M/N/Pc más join controls U+200C/U+200D, versión 17.
Incluye ñ, marcas combinantes y conectores; es una regla explícita del editor,
no segmentación UAX #29 por idioma ni posiciones de grafemas/LSP UTF-16.

La fuente legacy se decodifica con su página explícita, antes de comparar. Por
eso una consulta que no se puede codificar en esa página puede plegarse a una
letra que sí existe (p. ej. K frente a K). El reemplazo sigue usando encoding
estricto y rechaza caracteres no representables antes de editar.

## Evidencia local

Windows 11 Pro 10.0.26300, Ryzen 9 8945HS (8/16), Rust 1.85.1 MSVC, perfil de tamaño
existente, offline. **186 pruebas, 0 fallos**; fmt, Clippy workspace/all-targets
con `-D warnings` y release completos. Sin CI remoto.

En la revisión final se añadieron dos regresiones de propiedad de vista: Find/
Replace Current siguen el caret/selección de la vista activa, y F3 diferido se
descarta si cambia ViewId, caret o anchor. Se validaron de forma dirigida S0 (9),
F1 (8), Workbench (18), TUI (26), Clippy/release y ambos recorridos nativos. Son
188 casos distintos comprobados en total; no se repitieron F2 ni benchmarks
inalterados después de ese ajuste.

Se validó el bloque integrado: conformidad de todas las entradas default Unicode,
oráculo plano independiente frente a fragmentos 4/5/7/16, expansiones y límites
escalares, invalid UTF-8/BOM/CRLF, palabra, páginas legacy, revisiones/selección,
historial, Go to, cancelación que persiste al editar, deadlines, límites de entrada
y propuestas, undo/transacciones y regresiones de proyecto/guardado. TUI comprueba
que preparar/dibujar no escanea, marcadores, F3/selección, historial y posiciones.

`document_search_bench`: líneas ASCII de 8 bytes, 4 KiB/1/4/16 MiB, consultas sin
coincidencias; 5 muestras de calentamiento y 31 retenidas por tamaño. Percentiles
de schedule/fin/cancelación en índices 15/29; slots agregados ordenados al 95 %.
[Datos S0](benchmarks/S0-document.csv). En 16 MiB: schedule p50/p95 2/3 µs;
slot p95 417 µs; fin p50/p95 352732/401671 µs; cancelación p95 1 µs. Es tiempo de
modelo/procesamiento, no tecla-a-pantalla. La cancelación medida ocurre tras un
turno, no interrumpe una instrucción del decoder.

Se repitió el corpus S1 de cuatro archivos/2.097.144 bytes, 64 matches y 5+31
muestras: primer lote p50/p95 7815/9941 µs; fin 30633/37057 µs; cancel+join 48/65 µs.
[Comparación con matcher compartido](benchmarks/S0-project-shared.csv).
El p95 de fin subió de 27457 a 37057 µs respecto a S1 (~35 %). Se registra el
coste de decodificar/producir coordenadas en el engine común; no se afirma una
mejora de throughput. Ese coste se abordó después en el [bloque de optimización](S0_DECODER_OPTIMIZATION.md):
decoder prestado y memoria reutilizada, manteniendo un solo motor. Sus datos antes/
después y controles finales sustituyen esta medición como evidencia más reciente.

El ejecutable real pasó recorridos propios en ConPTY privada UTF-8: escribir
STRASSE y recibir un match de Straße sin otra tecla, F3, Ctrl+G escalar, reemplazo
sin guardar y undo; además volvió a pasar el flujo S1 de búsqueda/preview/confirmación.
[S0 nativo](benchmarks/S0-conpty.json), [regresión S1 nativa](benchmarks/S0-project-conpty.json).
El control previo de cursor dio p95 16,0726 ms hasta VT; no mide pantalla física.

PowerShell quedó detenido al inicializar su proveedor FileSystem en esta sesión.
Se continuó sin tocar configuración del sistema: Cargo directo con el mismo
toolchain/TEMP/entorno de proceso, y el mismo host C# compilado con csc del equipo.
Los helpers de junction usan ahora mklink /J con rutas de fixture entrecomilladas
vía variables del proceso; no necesitan arrancar PowerShell. El wrapper normal
continúa disponible. El cambio de licencia/datos y peso del ejecutable se informa
en [S0-binary.csv](benchmarks/S0-binary.csv); tamaño de archivo no equivale a RAM.

Siguen abiertos: regex con motor/presupuesto reales, corpus/manuales más amplios,
Linux, IME/grafemas, cold peaks, RAM global y latencia hasta pantalla física. No se
certifica <50 MiB ni p95 físico <16 ms, GUI, parser, LSP o AVM. La referencia visual
continúa siendo Sublime Text; Progress real M3, CI/Git con escritura M5 y plugins
externos post-M5. Las siguientes validaciones se agrupan por bloques amplios,
con comprobaciones intermedias solo para bloqueantes concretos.


## Superficies multilínea actuales

Los campos de Find/Replace ya admiten edición/pegado de saltos y navegación en
TUI y GUI, con gestos independientes de ejecutar y política EOL literal exacta.
El motor, cuotas/generaciones y transacción de reemplazo siguen siendo estos.
[Uso y evidencia conjunta](M2_INVISIBLES_MULTILINE.md); no nuevo motor regex.

## Regex implementado — continuación TUI, 2026-11-03

Regex real de documento y proyecto está integrado con regex-automata=0.4.18,
autorizado por Juan. Alt+R, historial, rangos originales, cancelación/cuotas y
reemplazo literal por transacciones; [contrato y evidencia](M2_REGEX_SEARCH.md).
Esta actualización sustituye las menciones históricas de regex deshabilitado.
124 casos distintos aprobados observados, cuatro flujos ConPTY y fmt/Clippy/
release locales. Dos fixtures junction y seis Git fallaron por creación/acceso
restringido: no se declara suite global aprobada ni aceptación física/RAM.

La instrucción vigente de Juan es continuar hasta terminar los pendientes TUI,
en bloques amplios, validar al cerrar cada bloque y activar el siguiente sin
esperar otro «continúa». Siguiente bloque: comparación de documentos abiertos
sin Git, desde el checkpoint revisado sobre el código actual. No restaurar fuentes
antiguas sobre resaltado/regex. Proveedores ABL/AVM y aceptación física conservan
sus requisitos reales; no afirmar capacidades que solo tengan contrato.

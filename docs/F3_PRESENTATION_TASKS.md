# F3 — Presentación, terminal incremental y tareas

Checkpoint local del 2026-11-02 sobre `e72856f`, rama `m2a/dual-pane-tui`.
Conserva documentos/vistas/transacciones de F1 y rangos/índices/cuotas de F2.
No añade dependencias de terceros ni cambia el perfil release.

## Actualizar antes de dibujar

`TuiApp::prepare_frame(&mut self, width, height)` calcula geometría, revela el
caret y prepara posiciones y rangos de texto visibles. Devuelve `Presentation`,
con lectura prestada de la aplicación y filas propias del viewport. Mientras
existe ese préstamo no se puede mutar la aplicación por su API de escritura.

`render(&Presentation) -> Screen` compone celdas sin revelar viewport ni consultar
las cachés textuales. Las pruebas comparan dos composiciones de la misma
presentación y comprueban que vistas y estadísticas de proyección permanecen
idénticas. `TuiApp::frame` es el helper que ejecuta las dos fases, también usado
por pruebas y mediciones; no constituye otro renderer.

Se preparan solo filas/columnas visibles de los grupos mostrados y posiciones de
resultados visibles. No se duplica la sesión ni el undo. Las dimensiones absurdas
que exceden 1.048.576 celdas se reducen a una presentación de 80×10. Esto acota el
frame, sin acreditar un límite global de memoria privada.

`FrameEncoder` compara celdas y cursor contra el último frame entregado con éxito.
Emite tramos contiguos cambiados, incluidos espacios para borrar texto y resets
de atributos para no acumular Reverse/Dim. Una pantalla idéntica no escribe ni
flushea; un cambio de cursor no repinta celdas. El primer frame, resize,
recuperación de foco, fallo de salida o invalidación explícita requieren frame
completo. No se conserva como baseline un frame cuya escritura/flush falló.

## Entrada y despertar

El loop principal captura un ticket de `Wakeup` antes de drenar eventos, procesa
hasta 32 entradas y 4 completions por vuelta y dibuja cuando hubo cambios. Antes
de dormir comprueba que no quede trabajo ya encolado. `Condvar` despierta por
entrada, completion o deadline; no espera otra tecla ni gira en reposo.

Un lector de terminal separado usa `crossterm::event::poll` con timeout de 100 ms
para permitir su cierre. La UI no hace ese polling. Su cola admite 64 eventos y
8 MiB de carga contabilizada; además puede existir un evento pendiente en el
lector. Paste excesivo se rechaza con un mensaje, sin insertar texto truncado.
Crossterm puede haber asignado ese paste antes de la admisión: no es una cuota
global del decoder de entrada. El cierre libera al productor aunque la cola esté
llena y espera su finalización antes de restaurar la terminal. Bracketed paste y
eventos de foco se activan/restauran junto con el modo raw y alternate screen.

## Contrato de trabajos internos

`jc-app::JobSystem` recibe un `JobKey`, documento, vista opcional, rango y proveedor
interno. Copia **solo el rango solicitado**, una vez al admitir el trabajo, a un
`TaskSnapshot` propio e inmutable. Incluye `DocumentId`, revisión, encoding y
`ViewId` opcional. La UI puede editar/cerrar mientras el worker lee esos bytes.
`DocumentRead` continúa siendo un préstamo; no se prolonga artificialmente ni se
clona el documento completo para mantenerlo vivo.

Cada admisión tiene `JobId` monotónico, que actúa como generación. Reemplazar la
misma clave cancela la anterior. En cada entrega se comprueban generación,
revisión, encoding y vida/asociación de la vista. Editar, cerrar/reabrir, cambiar
encoding o cancelar impide publicar resultados tardíos. Una instancia de
`JobSystem` se asocia a un solo Workbench; no se reutiliza entre proyectos/modelos.

Los workers reciben un puntero a función con snapshot, token cooperativo y salida
acotada. No reciben Workbench ni una referencia mutable al buffer. Devuelven
`Diagnostic`, `Message` o `ProposedEdit` con rangos absolutos dentro del snapshot.
Una salida inválida/excesiva invalida el lote completo, aunque el proveedor ignore
el error de `push`. La TUI envía diagnósticos/mensajes a Problems/Output y muestra
propuestas como texto; **no aplica ediciones automáticamente**. Una futura acción
aceptada deberá usar `apply_text_edits` con revisión esperada y un undo.

| Recurso | Valor por defecto |
|---|---|
| Workers | 2, creados al primer trabajo |
| Trabajos admitidos | 8, incluyendo pendientes, ejecutándose y completados sin entregar |
| Snapshot | 2 MiB por trabajo; 8 MiB admitidos en total |
| Resultado | 64 elementos y 64 KiB por trabajo |
| Deadline | 2 s desde admisión, incluyendo tiempo en cola |
| Entrega UI | Hasta 4 completions por vuelta |
| Problems / Output | 256 KiB cada uno, además de 1000 / 2000 líneas |
| Línea de log | 4096 bytes UTF-8, con marcador explícito de truncamiento |

`JobLimits` permite configurar límites dentro de máximos validados. La admisión
ocupada devuelve `Busy`; no crea otra cola ilimitada. Los resultados contabilizan
capacidad de strings y metadatos ocupados. Logs se recortan por bytes y líneas;
no son fuente canónica ni se usan para reescribir archivos. Cuotas lógicas no son
límites de RSS ni incluyen toda la memoria temporal de un proveedor.

La cancelación/deadline es **cooperativa**. Los proveedores internos deben revisar
el token y usar operaciones acotadas; no se puede interrumpir una función Rust
arbitraria que se bloquee e ignore el token. El cierre cancela y hace join de los
workers. Un panic se captura, descarta su salida y permite reutilizar el worker;
la terminal se invalida porque el panic hook puede haber escrito en stderr.
No es aislamiento de plugins no confiables ni host de procesos externos.

La base actual admite tareas ligadas a documentos. Las búsquedas de proyecto
necesitan además identidad/generación del contexto de proyecto y escaneo por
lotes con su propio presupuesto. Los cambios de configuración distintos del
encoding deberán cancelar/reemplazar la clave explícitamente. No se afirma que
esos proveedores ya estén conectados: no se inicia ninguno al abrir un archivo.

## Evidencia reproducible

Windows 11 Pro 10.0.26300, Ryzen 9 8945HS (8 núcleos/16 hilos), RAM visible
16.037.152 KiB, Rust 1.85.1 MSVC. Perfil existente `opt-level="s"`, thin LTO,
un codegen unit. Wrapper y dependencias offline; sin GitHub Actions.

- `fmt --all -- --check`: satisfactorio.
- `test --workspace --locked --offline`: **157 pruebas, 0 fallos**.
- `clippy --workspace --all-targets --locked --offline -- -D warnings`: satisfactorio.
- `build --workspace --release --locked --offline`: satisfactorio.

En una repetición apareció un fallo intermitente Windows 1175 de ReplaceFileW
en una prueba de sesión existente; la repetición dirigida pasó sin cambios.
La siguiente corrida completa pasó las 157 pruebas. Se conserva esa incidencia
y su causa no identificada en VALIDACION.

Las 15 pruebas nuevas cubren composición sin mutaciones, diff/no-op/cursor,
atributos, resize/fallo de salida, cola de entrada y cierre bajo presión,
admisión/resultados, snapshot durante edición, generaciones obsoletas,
encoding/cierre de vista/reapertura, cancelación/deadline/panic y entrega a paneles
sin entrada ni aplicación automática de propuestas. Las regresiones F1/F2 y de
archivo/encoding siguen incluidas en la corrida completa.

`cargo run -p jc-tui --example f3_pipeline --release --locked --offline` mide
Right → comando → preparación → composición → encoder VT en Vec. Cinco muestras
de calentamiento y 31 retenidas, p50/p95 por rango ordenado (índices 15/29).
Archivos ASCII de líneas de 8 bytes, 4 KiB/1 MiB/4 MiB, un grupo; 80×24 y 140×40.
Los p95 fueron 35–97 µs; frames estables emitieron 0 bytes y deltas medianos
23/41 bytes según viewport. **No incluye entrada OS, transporte ni pantalla.**
Datos: [F3-pipeline.csv](benchmarks/F3-pipeline.csv).

`cargo run -p jc-app --example f3_jobs --release --locked --offline` usa un mensaje
de prueba y otro proveedor que consulta el token entre sleeps de 1 ms. Mismo
calentamiento/muestras; entrega por Wakeup sin teclado. p50/p95 de admisión a
entrega 12/20 µs; cancelación de trabajo iniciado a entrega 1537/1567 µs. Es una
medición del contrato cooperativo, no de búsqueda real ni AVM.
Datos: [F3-jobs.csv](benchmarks/F3-jobs.csv).

Para la prueba nativa, compilar también el ejemplo `f3_console_host` en release y
ejecutar `scripts/smoke-conpty.ps1`. El host enlaza CONIN$/CONOUT$ explícitamente
porque este entorno ejecuta PowerShell con streams redirigidos. Usa una
pseudoconsola propia, fixture ASCII de 145 bytes en `work/`, 80×24, tres Right de
calentamiento y 31 medidos; reconstruye las celdas relevantes de la salida VT y
espera el nuevo `Col N`. Comprueba ausencia de clear en teclas, un frame completo
al pasar a 120×30 y salida por menú File/Exit. Limpia su fixture y procesos propios.
Las APIs de consola siguen el [contrato de Microsoft](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session).

Corrida registrada: p50/p95 **15,7802/16,0550 ms desde inyección hasta VT**,
1150 bytes para 31 Right, 0 clears en teclas, 1 tras resize, exit 0. Memoria privada
del proceso editor 1.089.536 bytes; CPU acumulada 0 ms en una ventana de reposo de
250 ms. Excluye conhost, bootstrap y anfitrión; ese muestreo corto y archivo mínimo
no acreditan <50 MiB global ni reposo universal. No incluye refresco/pintado del
terminal físico y **no cierra la meta p95 tecla-a-pantalla <16 ms**.
Datos: [F3-conpty.json](benchmarks/F3-conpty.json).

## Qué sigue abierto

F3 tiene implementación y evidencia local de sus mecanismos. La aceptación de
rendimiento global sigue abierta: latencia física, arranque, RAM/picos con varios
documentos y servicios, doble grupo nativo y corpus Unicode/legacy amplio.
La TUI todavía no ofrece ancho por grafemas/IME/DPI; no equivale a GUI terminada.

Search de documento sigue síncrono al reconstruir consultas; Explorer no migró
a este proveedor asíncrono. El siguiente incremento funcional es Search Plus de
proyecto con lotes cancelables y preview/conflictos sobre estos contratos, sin
indexador permanente. M2A continúa abierto. Prototipo GUI medido y dirección visual
Sublime Text según PLAN; ecode/eepp sigue como referencia operativa. Progress real
pertenece a M3, Git con escritura y CI remoto a M5, plugins externos a post-M5.

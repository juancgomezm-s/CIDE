# Entrega de estado Git por repositorio

**Estado vigente:** bloque integrado y validado por el integrador; ver el cierre al final. La presentación TUI de status sigue pendiente.

`crates/jc-app/src/git_status_tasks.rs` implementa `GitStatusController`, separado
del proveedor/parser de `git_status.rs`. Este bloque construye una API funcional
sin UI, comandos decorativos, watcher, escrituras Git ni snapshots de documentos.
La integración y las pruebas locales corresponden al cierre conjunto del bloque;
el trabajador no ejecutó Cargo, procesos Git, fixtures ni mediciones.

## Frontera y propiedad

El estado pertenece a un repositorio, no a una revisión de documento. Forzar un
`TaskStamp` documental o crear un documento ficticio para JobSystem daría una
identidad falsa. Se sigue la frontera ya existente de ProjectSearch: un worker
propio bajo demanda, con cuotas, cancelación, Wakeup y propietario que hace join.
No se introduce un host de proveedores externos. La inyección de proveedor es un
constructor privado usado únicamente por las pruebas del módulo.

El llamante elige un ejecutable instalado y confiable, la raíz absoluta y
`GitStatusReadOptions` (límites del transporte y del parser). `request` solo
valida metadatos, invalida la generación anterior y encola opciones propias; no
canonicaliza rutas, descubre repositorios, lee documentos ni ejecuta Git.
`GitReader::new`, descubrimiento/validación del repositorio, transporte y parser
se ejecutan dentro del worker. El reader es local a cada invocación; no existe un
proceso Git residente ni se comparte un reader mutable con la UI.

Los bytes de paths del resultado siguen en el modelo tipado del proveedor; el
controlador no los convierte con pérdidas. El estado refleja Git sobre el disco
y el index en ese instante, no las ediciones sin guardar de los buffers. No
promete que el repositorio permanezca inmutable después de la entrega.

## API y secuencia

| Operación | Contrato |
|---|---|
| `new(Wakeup)` | Sin hilo, proceso Git ni I/O; estado Idle. |
| `request(GitStatusOptions)` | Invalida resultados previos; activo se cancela cooperativamente y solo sobrevive la última petición pendiente. Retorna error tipado de admisión. |
| `refresh()` | Repite la configuración conservada; falla con NoConfiguration si nunca hubo una válida o un cambio detectado por poll la invalidó. |
| `cancel()` | Retira pendiente/completion/resultado, cancela activo y deja Cancelled. Conserva opciones para un refresh explícito. |
| `poll(&opciones_actuales)` | Rechaza cambios de raíz, ejecutable o cualquier límite; consume como máximo una completion y actualiza el estado sin I/O. Retorna cambio visible. |
| `ready()` | Indica una completion esperando poll; no significa que el estado aceptado Ready haya desaparecido. |
| `state`, `result`, `failure`, `root`, `message` | Lecturas prestadas; resultado y error tipados, mensaje estático acotado. |
| `Drop` | Cancela, señala parada, despierta worker y hace join. |

El integrador debe pasar a `poll` la configuración vigente antes de exponer
`result()`. Un resultado aceptado previamente también se retira si cambian las
opciones. Tras ese cambio queda Stale y se olvida la configuración antigua;
`refresh()` no puede volver silenciosamente a otra raíz. No se aplica una
canonicalización ni una comparación textual aproximada en la ruta de eventos:
la comparación estructural es conservadora y la validación real vive en reader.

Cada request obtiene una generación creciente comprobada. Una cancelación
desactiva además la generación admitida en la cola, incluso sin una nueva
petición. Así, un proveedor que devuelva éxito después de observar cancelación
no puede publicar esa entrega. Al agotarse u64 se cancela todo y se informa
GenerationExhausted, sin reutilizar identidades.

## Cotas y ciclo de vida

Hay exactamente un hilo reutilizable después de la primera petición admitida,
un proveedor activo, una petición pendiente de metadatos y una completion. El
worker no inicia otra petición mientras exista una completion sin consumir.
Pedir otra generación retira primero esa completion y reemplaza la pendiente;
no se acumulan snapshots ocultos mientras el consumidor deja de llamar poll.
Un resultado aceptado también se retira antes de una petición nueva.

Cada ruta de opciones está limitada a 4096 unidades de la representación nativa
de `OsStr`, tanto longitud como capacidad retenida de PathBuf. Una ruta corta
con reserva enorme también se rechaza antes de conservarla como pendiente.
Se utilizan las reglas únicas `GitStatusReadOptions::valid()` para
los límites de transporte/parser. El resultado y stderr mantienen las cuotas
del proveedor, incluso en fallos; el controlador no formatea stderr, paths o
payloads de panic en strings nuevas. Estas cotas lógicas no son RAM privada.

El deadline de lectura/parser pertenece al proveedor y empieza al ejecutar, no
al esperar turno. El token cooperativo empieza al tomar la petición y añade un
resguardo de 31 s frente al máximo de transporte de 30 s; permite distinguir un
TimedOut del proveedor de una cancelación explícita. Canonicalizar/abrir rutas y
la planificación del sistema no adquieren por ello una garantía física de
latencia. Drop espera la limpieza del proveedor/reader: no abandona threads ni
promete un join instantáneo. La contención y la recolección de procesos/hilos del
transporte siguen siendo propiedad de GitReader.

Wakeup señala inicio y finalización. Un panic del proveedor se captura, entrega
`GitStatusFailure::WorkerPanic` y termina en Failed; el hilo puede atender un
refresh posterior. La limpieza no requiere drenar una cola ajena ni depender de
una nueva tecla. La creación fallida del hilo retorna WorkerUnavailable.

## Regresiones preparadas y evidencia

Seis pruebas del módulo, sin Git real, cubren:

- Constructor lazy y rechazo de metadatos antes de iniciar un proveedor.
- Un activo, reemplazo de la pendiente y descarte de éxito antiguo tras cancelar;
  un gate con guard RAII fija el orden sin depender de sleeps.
- Cambios de raíz, ejecutable y límites sobre completion pendiente o resultado
  ya aceptado; refresh posterior no resucita la configuración invalidada.
- Cancelación de completion y refresh explícito con opciones retenidas.
- Errores tipados de cancelación, timeout, cuota y filtros, además de panic con
  recuperación del mismo worker.
- Cancelación y join al destruir un controlador con proveedor activo.

Canales y Wakeup coordinan las pruebas con plazos de fallo; el test de Drop usa
una espera cooperativa de predicado, sin exigir umbrales de rendimiento. No hay
afirmación de ejecución ni de validación nativa en este documento. El integrador
registrará la compilación, resultados reales y cualquier recorrido Git real al
cerrar el bloque coherente. UI/status, watcher, escrituras Git y aceptación global
de RAM/latencia quedan fuera de esta entrega.


## Cierre del integrador — 2026-11-02

Proveedor/parser, transporte compartido, controlador y ejemplo público integrados.
Una ventana tras construir todo: **261 pruebas workspace aprobadas, 0 fallidas**,
dos helpers de subproceso invocados por padres; formato, Clippy workspace/all-targets
con warnings denegados y release workspace bins/ejemplos pasaron. Son 15 pruebas
nuevas: seis parser, seis controlador y tres Git real. No hubo CI ni mediciones
de rendimiento adicionales. Los apartados de entrega del worker son históricos.

El fixture real comprobó MM/no rastreado por captura raw, proveedor tipado y
controlador real con Wakeup/poll/join. Index/config/worktree conservaron sus bytes,
sin index.lock. Rechazo de drivers clean/process sin ejecutar centinelas, raíz
padre/cancelación/salida excesiva. El controlador rechaza longitud Y capacidad de
PathBuf excesivas; una ruta corta con reserva enorme no elude la cuota de metadata.
Pruebas de cola/generaciones/configuración/panic/join usan coordinación explícita.

El ejemplo `cargo run -p jc-app --example git_status --release --locked --offline --
ABSOLUTE_GIT_EXECUTABLE ABSOLUTE_REPOSITORY_ROOT` usa la API pública; compila en
release y escapa cada byte de ruta sin conversión lossy ni apertura implícita.
No se ejecutó como otro benchmark: el recorrido real está en el fixture integrado.

La consulta representa configuración aislada y disco/index, no buffers sin guardar
ni necesariamente preferencias globales del Git personal. Filtros configurados,
renames detectados y cambios internos de submódulos quedan fuera de esta consulta.
Rutas byte no autorizan navegación filesystem. TUI status sigue pendiente;
diff activo HEAD/index ya conectado. Sin Git writes/watcher/GUI ni aceptación
global de RAM/pantalla física.

## Integración posterior — 2026-11-02

La [presentación Git status TUI](GIT_STATUS_PRESENTATION.md) ya está conectada:
consulta explícita sin documento, lista visible acotada, selección y refresh/cancel,
sin abrir paths ni escribir Git. Cierre posterior: 290 pruebas completas locales,
fmt/Clippy/release. Las menciones anteriores de UI pendiente son históricas;
se conservan límites del proveedor y aceptación global RAM/pantalla abierta.

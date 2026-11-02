# Git diff — controlador y entrega versionada

Bloque construido contra [el contrato compartido](GIT_DIFF_DELIVERY_CONTRACT.md),
2026-11-02. Propiedad del worker: `git_diff.rs`, `tests/git_diff.rs` en jc-app y este
documento. El integrador mantiene JobSystem, OwnedDiffModel, exports, comandos,
frontend, plan, validación y commits. **Código/pruebas preparados; Cargo, Git y
fixtures no fueron ejecutados por el worker.** La evidencia final corresponde al
cierre serial del integrador.

## Propiedad y flujo

Existe un `GitDiffController` por Workbench/JobSystem, con clave reservada
`GIT_DIFF_JOB_KEY`. No crea un pool, canal, indexer ni cola de fuentes propios.
Conserva como máximo un JobId en vuelo, una intención pendiente de **solo metadatos**
y un resultado aceptado. Una nueva solicitud descarta inmediatamente el resultado
anterior, incrementa generación y cancela el trabajo anterior; reemplaza la intención
pendiente sin capturar otro documento mientras el JobId anterior no sea consumido.
Incluso cancel/clear conservan ese JobId hasta su completion, porque la limpieza
del proceso Git puede prolongar la cancelación cooperativa.

`request` valida identidad/encoding/vista y tamaños sobre datos ya disponibles.
`tick` invalida resultados o intenciones obsoletas e intenta admitir la pendiente.
Ninguno ejecuta Git, canonicaliza ni calcula diffs. Únicamente
`JobSystem::submit_git_diff` captura una copia entera acotada de la revisión, una
vez admitida. Si el sistema está ocupado, la intención sigue Queued sin otra copia;
si una reserva individual no cabe en la configuración, falla Limited sin esperar
indefinidamente ni iniciar workers. El controlador no drena completions ajenas.

La ocupación temporal del presupuesto compartido de snapshots por tareas F3 Plain
también mantiene Git en Queued: no se confunde con un tamaño individual excesivo.
Una regresión local usa presupuesto individual/total de cuatro bytes y una
tarea Plain gated que los retiene. Comprueba que Git no copia ni reserva otro
artefacto mientras espera, incluso cuando Plain terminó sin poll; después de
consumir ese completion se admite una sola captura Git y llega Ready. Aprobada
en el control dirigido de nueve pruebas Git y ocho F3 jobs; Clippy de aplicación
y release workspace repetidos tras esta corrección. La semántica Plain se conserva.

El worker `run` recibe TaskSnapshot y Cancellation. Resuelve rutas, obtiene el blob
con GitReader, mueve sus bytes a `Arc<Vec<u8>>` y comparte el Arc del snapshot de
documento. OwnedDiffModel usa el mismo motor puro existente. No se clona un
Document/FileSession/historial, no hay edición ni escritura Git/disco productiva.
La fuente derecha es el documento **incluidos cambios sin guardar**, no una nueva
lectura del archivo. La izquierda queda identificada por blob OID y commit HEAD
cuando corresponde; index conserva el OID/modo seleccionado. No existe watcher
ni promesa de que HEAD/index sigan apuntando al mismo objeto al mostrarlo.

## Validez de entrega

`accept` devuelve true para completions de su clave, también las descartadas, y
false para otras claves sin tocar su output. Libera el slot en vuelo solo para su
JobId. Acepta el resultado únicamente si coinciden generación, TaskStamp del job y
reply, revisión, encoding, DocumentId, ViewId activo y ruta guardada en FileSession,
además de `CompletedTask.current/applicable`. Una revisión/vista que siga existiendo
en segundo plano no permite navegar otra vista activa por accidente.

`tick` devuelve cambio visible y debe ejecutarse antes de preparar presentación.
`result()` representa lo válido en la última comprobación, por lo que el frontend
debe conservar ese orden. Cambiar proyecto, cwd o ejecutable Git requiere
clear/request explícito desde el frontend: Workbench no posee ese contexto externo
y el controlador no intenta detectarlo mediante I/O dentro del renderer.

Estados: Idle, Queued, Running, Ready, Limited, Cancelled, Stale, Failed y TimedOut.
No se publican alineaciones parciales como Ready. Los límites del diff descartan
el artefacto; errores y cancelación tampoco ofrecen hunks navegables. JobSystem
traduce deadline/pánico/fallos de proveedor. El timeout propio de GitReader y sus
límites se transportan como mensajes internos constantes exactos dentro del reply
de error definido por el contrato; no se clasifica stderr ni texto externo por
substring. El detalle de un error no excede 512 bytes UTF-8 y no imprime el fuente.

## Navegación sin edición

`select_relative` limita la selección a hunks existentes. `select_hunk` comprueba
otra vez la intención vigente antes de usarla y solo llama a navegación compartida
de Workbench. Usa el inicio del rango derecho, o EOF para un borrado al final;
un primer rango que incluye BOM UTF-8 lleva el caret al byte 3, sin entrar dentro
del BOM. Legacy no aplica ese ajuste. No convierte DiffLine a columna visual,
grafema o LSP ni inventa una correspondencia exacta para líneas cambiadas.

Seleccionar un hunk no publica revisión, transacción, guardado ni undo. Una ruta
cambiada por Save As, un cierre, otra vista/documento, revisión o encoding distinto
invalidan la acción; se exige refresh. Navegar a una ubicación no vuelve editable
el snapshot HEAD/index.

## Rutas y cancelación

Solo el worker canonicaliza raíz y documento. Si el archivo ya no existe, admite
un nombre bajo un parent existente validado; no inventa una jerarquía inexistente.
La ruta resuelta debe permanecer bajo la raíz y producir componentes relativos
normales; GitReader vuelve a aplicar su validación literal. Un archivo/junction
que resuelva fuera se rechaza antes de lanzar Git. Las rutas/contexto solicitados
tienen cota de 4096 bytes cada uno.

No se afirma sandbox frente a cambios simultáneos de nombres, `.git`, alternates
o configuración. Se mantienen los límites y garantías reales de
[GitReader](GIT_READ_ONLY_PROVIDER.md): ejecutable de confianza, entorno controlado,
proceso Windows contenido, salidas acotadas y limpieza RAII sin abandonar lectores.
La canonicalización y teardown del kernel no son preemptibles. Cancelar cambia la
intención inmediatamente, pero la reserva permanece hasta que termine y se consuma
el trabajo; no es un deadline duro de liberación de recursos.

## Cuotas y memoria

| Recurso | Cota |
|---|---:|
| Snapshot de documento entero | 2 MiB |
| Blob Git | 2 MiB |
| Entrada total del diff | 4 MiB |
| Trace/índices/spans del diff | 4 MiB lógicos |
| Reserva de artefacto desde admisión hasta poll | 12 MiB |
| Git diff admitido | Uno por controlador |
| Intención pendiente | Una, solo metadatos |
| Deadline del job / GitReader | 10 s / 5 s cooperativos |
| Error presentado | 512 bytes UTF-8 |

`GitDiffReply::memory_bytes` cobra capacidades de ambas fuentes/modelo, Arc y
vectores mediante OwnedDiffModel, paths canónicos, ruta lógica, OID y error. El
cargo es conservador, no RAM privada. JobSystem retiene su reserva también para
un completion sin poll; al consumirlo la reserva se libera. El controlador posee
entonces el resultado aceptado hasta invalidarlo/clear; request lo descarta antes
de admitir otro snapshot. Procesos Git, stacks, buffers de I/O y allocator tienen
costes propios que estas cuotas no convierten en una garantía global de RAM.

## Pruebas preparadas

```console
cargo test -p jc-app --test git_diff --locked --offline -- --test-threads=1
```

Los fixtures crean repositorios sintéticos exclusivos bajo
`work/test-temp/jc git diff PID-secuencia/repo`, con entorno saneado. Sus únicos
comandos con escritura (init/hash-object/update-index/write-tree/commit-tree/
update-ref) preparan datos allí; nunca operan sobre el repositorio del usuario.
Save As de la matriz también escribe solo en ese fixture. La limpieza verifica
raíz canónica y parent exacto antes de borrar. No se descarga Git ni se usa CI.

Casos observables: dirty snapshot frente a HEAD/index sin guardar; BOM/UTF-8
inválido y legacy byte-preserving; selección sin cambiar revisión/undo; borrado
a EOF; diez intenciones con un único snapshot/reserva en vuelo y solo la última
admitida después del acknowledgement; cancel/clear/retry sin resurrección; reserva
12 MiB retenida con completion sin poll; entrega rechazada por edición, encoding,
vista, documento, Save As y cierre; límites de documento/reserva antes de copiar o
crear workers; escape de raíz y ejecutable ausente resueltos solo en worker.

Validación del integrador: 237 pruebas locales aprobadas al cierre, fmt, Clippy
workspace/all-targets y release. ConPTY verificó HEAD/index sin otra tecla,
navegación, undo→Stale, hunks conservados al ocultar el panel inferior y disco/index
sin cambios: [datos crudos](benchmarks/M2-git-conpty.json). Véase VALIDACION para
las correcciones de integración y los controles afectados repetidos.

Pendientes: rendimiento/RAM editor + auxiliares, estado de todo el repositorio, otras plataformas
y aceptación global de M2A. Este bloque no implementa staging/commit/merge ni GUI.

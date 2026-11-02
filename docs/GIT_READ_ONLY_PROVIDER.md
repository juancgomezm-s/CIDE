# Proveedor Git — snapshots de blobs de solo lectura

Frente preparado por el worker `diff_read_model`, 2026-11-02. Propiedad exclusiva:
`crates/jc-app/src/git_read.rs` y este documento. El integrador conserva exports,
manifests, comandos/UI, PLAN, VALIDACION, compilación y commits. Código nuevo,
sin port ni dependencia adicional. No se ha ejecutado Git/Cargo ni fixtures por
el worker. El integrador completó compilación, pruebas y controles locales
descritos al final; no hay conexión de UI ni entrega por jobs todavía.

## Alcance y API

`GitReader::new(executable_absoluto)` recibe el Git instalado y elegido por el
caller. No busca un programa en PATH, descarga herramientas ni ejecuta nada al
construirse. El proveedor está implementado solo para **Windows 10+**; fuera de
Windows devuelve `UnsupportedPlatform`. Soporte/validación de otros sistemas queda
pendiente, sin fallback de proceso sin contención.

`read_blob(raíz_absoluta, Head|Index, ruta_relativa, límites, cancelled)` devuelve
un `GitBlobSnapshot` privado e inmutable. `bytes()` entrega solo un préstamo; no
existe acceso mutable, guardado ni clon de `Document`/`FileSession`/undo. El Debug
del snapshot muestra identidad y longitud, sin imprimir el fuente completo.

La identidad contiene raíz y git-dir canónicos, ruta lógica, origen, blob OID,
modo regular/ejecutable y commit OID para HEAD. La selección de HEAD se fija primero
a un commit y luego se busca el blob en ese commit. Para index se conserva el
blob/modo stage 0 seleccionado por ruta; no se afirma una versión global del índice
ni que siga seleccionándolo después. Los bytes permanecen válidos para ese OID
aunque HEAD o index cambien posteriormente. El resultado es apto para prestar a
`DiffModel`; el coordinador futuro aún debe validar revisiones/generaciones al
entregarlo junto al snapshot del documento de trabajo.

No hay status, watcher, indexer, menú, comando, renderer, staging, commit, checkout,
merge, fetch, pull o push. Su presencia no acredita una vista Git funcional ni cierre
de M2A. Las operaciones Git con escritura siguen en M5 por acción explícita.

## Lectura exacta y selección

El proveedor usa solo rev-parse, ls-tree, ls-files y cat-file. Descubre/verifica una
raíz de repositorio no bare; no acepta silenciosamente el padre de una subcarpeta.
Fija después `--git-dir`/`--work-tree` a la identidad canónica descubierta.

Las entradas de árbol/índice se reciben con `-z` y se comprueba la ruta exacta;
index debe ser stage 0. Conflictos, ausencia, directorios, symlinks/submodules y
protocolos inválidos son errores explícitos. El modo permite solo 100644/100755.
Se admiten OID completos SHA-1/SHA-256, sin aceptar OID nulo.
Los formatos NUL/stage proceden de [ls-files](https://git-scm.com/docs/git-ls-files).

Se consulta primero el tamaño y después `cat-file blob <OID>`. No hay textconv,
smudge/clean, conversión EOL, pretty-print ni decodificación de bytes de fuente;
el tamaño real debe coincidir. BOM, bytes inválidos, legacy y newline final siguen
siendo bytes originales del objeto. Véase [cat-file](https://git-scm.com/docs/git-cat-file).
El encoding para mostrar esos bytes deberá elegirlo el documento/proyecto.

Las rutas se limitan a 4096 bytes UTF-8 y componentes normales relativos. Se
rechazan absoluto/UNC/drive, componentes vacíos/dot/parent, `.git` sin distinción
de mayúsculas, nombres de dispositivo Windows, ADS/colon, caracteres de nombre
ambiguos, NUL/CR/LF y componentes con punto/espacio final. No se hace una conversión
lossy de nombres: rutas que no pueden representarse en UTF-8 se rechazan. Los bytes
del contenido no tienen esa restricción. `niño[1].p` es un literal, no un glob.

Esta validación selecciona objetos del repositorio; **no es un sandbox hostil**.
No prueba aislamiento ante actores que cambien directorios, `.git`, commondir,
config/alternates u objetos mientras se lee. Un git-dir de worktree puede estar
fuera de la raíz de archivos. No se afirma CAS ni seguridad completa de raíz por
canonicalizar. El ejecutable Git y el repositorio deben ser de confianza.

## Proceso, entorno y límites

Los argumentos son una lista estructurada serializada una vez en la frontera
CreateProcessW mediante quoting CRT; no hay shell. Se suministra ruta absoluta del
ejecutable y directorio de trabajo explícito. El bloque de entorno nuevo conserva
solo SYSTEMROOT/WINDIR/TEMP/TMP y valores Git controlados; no hereda GIT_DIR,
GIT_INDEX_FILE, TRACE, config injections, SSH ni credenciales del proceso llamador.
Config global/system se deshabilita solo en el proceso. No se escribe configuración.

Se fijan `--no-pager`, `--no-replace-objects`, `--no-optional-locks`, `--no-lazy-fetch`
y `--literal-pathspecs`. Git debe entender esas opciones: una versión que no lo
haga falla sin reintento inseguro. Se deshabilitan fsmonitor, hooksPath,
untrackedCache, auto-gc/maintenance y credential helper mediante `-c` por proceso;
no se invocan diff, filtros o proveedores externos. La ausencia de lazy fetch
evita obtener objetos remotos como efecto de una lectura local. Opciones documentadas
en [Git](https://git-scm.com/docs/git).

Un Job Object privado usa kill-on-close sin breakaway. STARTUPINFOEX lleva una
HANDLE_LIST con **solo tres pipes propios** y una JOB_LIST; Windows asigna el job
antes de permitir correr el hilo inicial. Si no se puede establecer esa contención,
no se lanza un proceso sin ella. No se suspenden procesos externos ni se usan NT APIs
no documentadas. Esta asociación previa y la herencia restringida están descritas en
[UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

Dos lectores drenan stdout/stderr simultáneamente con límites; no se espera un
`Command::output` ilimitado que pueda bloquear por llenar el otro pipe. El loop
consulta cancelación/deadline y desbordamientos, esperando al proceso en turnos
de hasta 5 ms. Un controlador RAII posee job, proceso y lectores desde el arranque:
todas las salidas posteriores a CreateProcess, incluidos fallos de crear el primer
o segundo lector y el unwinding de un pánico del callback, pasan por su limpieza.
Primero cierra el job, terminando miembros/descendientes; comprueba terminación de
los lectores antes de unirlos y conserva el handle del proceso hasta observar su
salida. El cierre del último handle del job termina sus miembros, según
[Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).

En éxito permite drenar stdout/stderr completos hasta EOF sin marcar parada ni
cortar bytes pendientes. Si ese drenaje no concluye durante la gracia de cinco
segundos, rechaza la captura y pasa a cancelación; esa gracia **no es una cota del
retorno**. En cancelación/fallo marca parada, usa CancelSynchronousIo solo sobre
sus hilos y repite si ERROR_NOT_FOUND coincidió con un intervalo entre lecturas.
Cada lector comprueba parada antes de su siguiente read. No se sueltan JoinHandles
ni se separan threads para cumplir un timer: la limpieza conserva propiedad y
puede prolongarse hasta que el sistema termine el I/O/proceso. Microsoft documenta
la carrera de cancelación y sus límites en
[cancelación de I/O](https://learn.microsoft.com/en-us/windows/win32/fileio/canceling-pending-i-o-operations).
No hay garantía frente a un kernel/driver que no complete esa cancelación ni
promesa de recuperación de un proceso Rust configurado para abortar al hacer panic.

| Recurso | Predeterminado | Máximo configurable |
|---|---:|---:|
| Blob devuelto | 2 MiB | 16 MiB |
| stdout de metadatos por comando | 64 KiB | 1 MiB |
| stderr por comando | 16 KiB | 64 KiB |
| Deadline de la solicitud completa | 5 s | 30 s |

El deadline cubre el conjunto de comandos, no se reinicia por subcomando. Se
distinguen Cancelled, TimedOut, límites de Blob/Metadata/Stderr, fallos de contención,
I/O y salida Git no cero. El diagnóstico stderr se conserva como bytes acotados
cuando falla un comando; no se mezcla con el fuente.

Las cotas de salida no limitan RAM del proceso Git, lectura del índice o costes de
descompresión internos. Los lectores y el allocator tienen costes propios. La
canonicalización del filesystem y llamadas del sistema no son preemptibles; el
deadline es de control cooperativo con terminación del job, no garantía dura de
tiempo real. La espera de limpieza puede prolongar el retorno. Se debe ejecutar
este trabajo fuera de entrada/render y medir editor/auxiliares por separado.

## Validación local ejecutada

El integrador declaró `pub mod git_read` desde lib.rs y ejecutó en serie los
controles con Windows 11 Pro 10.0.26300, Rust 1.85.1 MSVC offline y Git instalado
**2.54.0.windows.1**. No se modificaron manifests/lockfiles. Comandos dirigidos:

```console
cargo test -p jc-app git_read --locked --offline -- --test-threads=1
cargo clippy -p jc-app --all-targets --locked --offline -- -D warnings
```

Los tests necesitan Git ya instalado (`JC_GIT_TEST_EXE` permite señalar su ruta
absoluta; de lo contrario el fixture lo localiza en PATH). Preparan repositorios
sintéticos exclusivos bajo `work/test-temp/jc git read PID-secuencia`. Solo ahí usan
init, hash-object -w, update-index, write-tree, commit-tree, update-ref y config para
crear datos; **no son acciones de producción del proveedor ni operaciones sobre
el repositorio de JC-IDE**. Init usa template vacío; commit-tree evita hooks de
porcelain. La limpieza verifica raíz canónica y parent exacto antes de borrar.

Casos: blobs HEAD/index diferentes e inmutables, bytes legacy/BOM/EOL exactos,
rutas Unicode/espacios/corchetes literales, modo y OID, ausencia/conflictos/symlinks,
blob vacío, rechazo de rutas/descubrimiento de padre, límites de salida y ausencia
de escritura del index/config/worktree. Configuran proveedores ficticios que no
deben invocarse. Otros casos lanzan el **propio binario de tests** para comprobar
timeout/cancelación, salida excesiva y terminación de un descendiente que retiene
pipes. Inyecciones deterministas de fallo del primer/segundo lector y un callback
que hace panic después de CreateProcess comprueban, mediante un handle OS
independiente, que el proceso creado ya terminó al retornar o capturar el unwind.
Una captura exitosa de un blob de 1 MiB + 31 bytes comprueba drenaje completo.
Dos tests ignorados son fixtures de subproceso invocados explícitamente por
los tests de control; no constituyen casos omitidos de producto. El parser de
argumentos Windows comprueba el roundtrip de espacios, comillas, backslash y Unicode.

Resultado: nueve pruebas efectivas aprobadas; suite completa del workspace
**218 pruebas, 0 fallos**, con dos helpers de subproceso ignorados como entradas
normales pero invocados explícitamente por sus tests de control. fmt --check,
Clippy workspace/all-targets -D warnings y release completos, sin CI remoto.
La revisión encontró dos rutas sin reap confirmado al fallar un reader; se
reemplazaron por el guard RAII y se verificaron fallos/pánico con handles OS.

El integrador añadió al caso HEAD/index existente la composición real con DiffModel:
dos fuentes con bytes inválidos/BOM/EOL distintos producen un hunk exacto sin
normalizar ni escribir. Se ejecutó de forma dirigida después de la suite; Clippy
pidió retirar un drop(control) innecesario en otro test. Cambio solo de test y
controles finales aprobados, sin repetir suites o benchmarks inalterados.

Pendientes: medición de RAM/latencia del proceso Git y sus lectores bajo carga,
integración con JobSystem/TaskStamp, entrega generacional del diff, UI y Git status.
No se atribuyen al Git activo las mediciones de RAM del editor con servicios apagados.
Windows 10 es requisito de la API, no plataforma local validada; Linux/M5 sigue
sin validar. La limpieza de kernel puede prolongar retorno: no se promete una
cota dura por el deadline o por la gracia de drenaje.

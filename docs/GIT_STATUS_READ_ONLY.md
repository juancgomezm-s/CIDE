# Estado Git — proveedor acotado de solo lectura

**Estado vigente:** bloque integrado y validado por el integrador; ver el cierre al final. La presentación TUI de status sigue pendiente.

Estado de construcción: `git_status.rs` preparado, sin Cargo ni fixtures
ejecutados por el worker. La validación corresponde a una ventana serial del
integrador; este documento no afirma que las pruebas ya hayan pasado.

## Propiedad y API

`read_status(&GitReader, raíz, GitStatusReadOptions, cancelled)` es una operación
síncrona para un worker. Reutiliza el transporte contenido de `git_read`, su
descubrimiento estricto de raíz y el deadline de la solicitud. No crea procesos
con otro runner, pool, watcher, indexer, fuente editable ni historial.

`GitStatusReadOptions { git: GitReadLimits, parse: GitStatusLimits }` permite
validación sin I/O mediante `valid()`. `GitStatusSnapshot` posee identidad del
repositorio y `GitStatus`; ofrece solo préstamos y contabilidad lógica. El
constructor `from_parts` permite componer fixtures/observaciones: no certifica
que el estado siga vigente ni una versión global CAS de HEAD/index/worktree.
La futura entrega generacional pertenece al controlador, no al parser.

Los parsers puros públicos `parse_porcelain_v2` y `parse_porcelain_v1` aceptan
bytes y cancelación. Las entradas distinguen cambios de index/worktree,
conflictos tipados, no rastreados e ignorados. Un conflicto conserva su categoría
de merge, sin reinterpretar sus dos letras como cambios staged/unstaged.
Rename/copy retiene destino, origen y porcentaje cuando existe; los campos de
submódulo de v2 también son tipados. Modos/OID se validan como campos del
protocolo, pero no se conservan como una promesa de versión por archivo.

Las rutas son copias propias exactas en `Box<[u8]>`; no existe conversión lossy,
unquote, normalización, `PathBuf` ni apertura implícita. El Debug muestra longitud
de ruta, sin imprimir sus bytes/control. Se rechazan componentes vacíos, `.`,
`..`, `.git` y ruta absoluta con `/`; se admite slash final en no rastreados e
ignorados como metadato de directorio. Bytes no UTF-8, tab, CR/LF, comillas y
backslash permanecen exactos. Este tipo no autoriza navegación ni demuestra
que una ruta sea representable/segura en Windows: otro consumidor deberá decidir
esa conversión explícitamente y escapar controles antes de mostrarla.

La delimitación NUL, separación origen/destino y estados provienen de la
[documentación de git-status](https://git-scm.com/docs/git-status). En v2 se
ignoran headers desconocidos acotados y se admiten registros ordinarios,
rename/copy, conflicto, no rastreado e ignorado; un registro desconocido o
malformado rechaza toda la captura. No se ofrece una lista parcial como completa.

## Alcance efectivo de la lectura productiva

El transporte nuevo del integrador es `GitReader::read_status_porcelain_v2`,
visible dentro del crate. Usa formato v2 NUL, enumera archivos no rastreados,
excluye ignorados, desactiva detección de renames y no recorre cambios de
submódulos (`--no-renames`, `--ignore-submodules=all`). Por eso **el parser puede
representar más estados que esta primera consulta solicita**: un movimiento
puede aparecer como borrado/añadido y no se promete información completa de
submódulos. No se pasan pathspecs derivados de las rutas recibidas.

Se conserva el entorno aislado del lector: configuración global/system y
variables Git heredadas deshabilitadas, sin escritura de configuración. El
resultado representa ese entorno, que puede diferir del Git personal por
`core.autocrlf`, atributos o filtros definidos globalmente. Se conserva la
configuración local aplicable que no haya sido restringida explícitamente.
No se afirma equivalencia con todas las preferencias del usuario.

`status` no es inocuo solo por ser una consulta: al inspeccionar contenido puede
necesitar conversiones. Los drivers clean/process pueden ejecutar programas.
Este incremento hace preflight de sus definiciones efectivas con `git config`
y rechaza **cualquier** definición de `filter.*.clean` o `filter.*.process` con
`GitReadError::ConfiguredFilter`, incluso vacía o sin archivo aplicable. No
ejecuta ni sustituye esos drivers. Repositorios con esos filtros no están
soportados por este primer proveedor. El integrador prepara fixtures con
drivers centinela para comprobar que no se invocan; no se han ejecutado aquí.

Se investigó `GIT_ATTR_SOURCE`: el selector cambia de qué árbol se leen atributos,
no es una garantía de desactivar todos los programas. Forzar otro árbol o
suprimir atributos/clean en general cambia las conversiones contra las que Git
compara el contenido, pudiendo cambiar el estado observado. Esa inferencia se
basa en [Git: attr-source](https://git-scm.com/docs/git) y
[atributos y filtros](https://git-scm.com/docs/gitattributes). No se implementa
ese bypass ni se presenta una semántica alterada como status normal.

La raíz, el ejecutable y la configuración deben ser de confianza y permanecer
estables durante la operación. El preflight no elimina una carrera si otro actor
cambia configuración/inclusiones después. No es sandbox frente a repositorios
hostiles ni una garantía de no ejecutar filtros ante mutación concurrente.

## Cuotas, tiempo y memoria

| Recurso del parser | Predeterminado | Techo configurable |
| --- | ---: | ---: |
| Entrada bruta | 1 MiB | 1 MiB |
| Entradas devueltas | 4096 | 16384 |
| Registros NUL, incluidos headers/orígenes | 8256 | 32896 |
| Bytes por ruta | 4096 | 4096 |
| Capacidad lógica propia | 2 MiB | 4 MiB |

La reserva cuenta raíz del resultado, capacidad real del Vec de entradas y
longitudes de las rutas boxed. En `read_status` se resta primero el coste de
identidad/raíz del snapshot, incluidos los `PathBuf` de identidad. El crecimiento
del Vec es geométrico y limitado por entradas/presupuesto; el allocator no se
confunde con una garantía de RAM. `memory_bytes()` permite al coordinador cargar
la observación a su reserva de artefacto.

Durante el parseo pueden coexistir la captura cruda y la representación propia:
el pico incluye hasta la cuota de stdout del transporte **más** la cuota propia,
además de stderr, buffers de readers y overhead del allocator. El presupuesto
de resultado no limita RAM del proceso Git ni sus costes internos al recorrer
archivos. Las cuotas del transporte permanecen en `GitReadLimits`; su captura de
metadatos se eleva a 1 MiB por defecto para este proveedor, sin cambiar read_blob.

El deadline empieza antes del transporte. El callback comprueba ese mismo plazo
durante discovery/config/status y el parser conserva la fecha límite original.
No se concede otro timeout completo tras capturar stdout. Cancelación y timeout
son errores distintos, sin clasificar texto externo. El parser comprueba control
por registro y por cada tramo de hasta 4096 bytes al buscar NUL; los tokens fijos
del protocolo también tienen longitud acotada. Al fallar se descarta el resultado.

Se mantienen las garantías reales del [runner](GIT_READ_ONLY_PROVIDER.md):
`--no-optional-locks`, sin lazy fetch, proceso Windows contenido antes de correr,
handles restringidos, stdout/stderr acotados, cierre RAII y reaping. La limpieza
de kernel y canonicalización no son preemptibles; no se promete deadline duro
de retorno. No se introducen Git writes, credenciales, hooks, shell del editor,
watcher ni indexer permanente.

## Regresiones construidas y pendientes

Seis tests puros del módulo cubren conservación/inmutabilidad de rutas con bytes
inválidos y controles, estados dobles, headers desconocidos, SHA-256/nulos,
metadatos de submódulo, rename/copy de v1/v2, siete conflictos, truncamientos,
tokens malformados, rechazo sin lista parcial, límites por recurso y cancelación
durante un registro largo. El plazo vencido se comprueba de forma determinista.
Sus fixtures son bytes propios en memoria y no ejecutan Git ni crean repositorios.

El integrador añade casos reales en el fixture exclusivo existente de git_read:
estado MM/no rastreado, conservación de index/config/worktree, rechazo de filtros,
raíz incorrecta, cancelación y cuota de salida. Solo la preparación de esos
repositorios de prueba puede escribir objetos/índice propios. Este worker no
ejecutó fixtures ni validaciones y no ha declarado esos casos aprobados.

Integración pendiente al entregar: export del módulo, compilación/pruebas en una
ventana serial, consumo por tarea versionada y presentación no decorativa desde
resultados vigentes. No hay aceptación de Git con escritura, GUI, memoria global
ni cierre de M2A por la existencia de este proveedor.


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

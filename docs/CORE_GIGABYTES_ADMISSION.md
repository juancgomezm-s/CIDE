# Archivos de gigabytes — auditoría y admisión propuesta

2026-11-03. **El editor completo todavía no admite documentos de varios GB.**
Hay una base local de lectura paginada y guardado streaming, detallada abajo.
Este documento responde a la nueva prioridad de Juan: el núcleo debe manejar
archivos inmensos, incluidos archivos que crucen4GiB. La auditoría inicial de las
secciones siguientes es histórica; sus fórmulas son estimaciones de estructura.

## Incremento construido después de la auditoría

`jc-core::source::ImmutableByteSource` ofrece lecturas fallibles de hasta64KiB
con leases `Arc`. `jc-files::paged_source::PagedFileSource` implementa lectura
posicional Windows x64 y caché de páginas con cuota. Las páginas prestadas siguen
contando; si están fijadas y no cabe otra, devuelve Busy antes de reservarla.

La prueba local usó un archivo disperso de5GiB lógicos (196608B asignados en disco),
leyó262144B en cuatro llamadas y accedió por encima de4GiB. Validó reemplazo del
nombre, concurrencia y rechazo de una vista previa de escritura mapeada. Esto
prueba la fuente aislada, no el editor, su RAM total ni su latencia física.

Guardar y comprobar cambios externos ya usan streaming de64KiB: se retiraron
`to_bytes` y `fs::read` de esas rutas. Una prueba verifica que el conflicto ocurrido
mientras se prepara el temporal no adopte la versión externa como baseline.
La apertura actual y las arenas del documento siguen residentes en memoria.

**Política elegida por Juan: instantánea privada en disco**, que permitirá que otros
programas cambien el original mientras permanece abierto. Falta construir ese
copiador y conectar fuentes paginadas, lecturas fallibles e índices progresivos a
Document/Workbench. El prototipo de handle protegido es evidencia técnica, no la
política final de apertura. La recomendación inicial de origen protegido más abajo
queda sustituida por esta elección. La recuperación durable se desarrolla en Rust
como se describe en `SESSION_RECOVERY.md`; sus cuotas actuales tampoco admiten GB.

## Conclusión

Conservar F1, las transacciones y la tabla de piezas es viable. El límite principal
no es que una pieza sea grande: hoy el original es un `Vec<u8>`, abrir/guardar
materializan todo el contenido y la primera proyección indexa todas las líneas.
Cambiar solamente el límite de apertura o añadir mmap dejaría esos costes activos.
La entrega mínima correcta necesita fuente original inmutable respaldada por
archivo, lecturas por páginas con errores explícitos, índice textual progresivo
y guardado por streaming. No crear un segundo editor «para archivos grandes».

La primera plataforma propuesta es Windows/MSVC **x64**. En32bits se debe rechazar
explícitamente una longitud no representable, sin cast/truncamiento ni intentar
una reserva contigua de varios gigabytes. La API pública existente usa `usize`;
los offsets del archivo y su longitud de metadatos deben conservar `u64`, con
conversión comprobada a las coordenadas del documento x64.

## Obstáculos localizados en la auditoría inicial

| Componente y punto | Comportamiento observado | Consecuencia GB |
|---|---|---|
| `jc-files/src/lib.rs`, `FileSession::open` | `fs::read` completo; luego `probe_encoding` y fingerprint | Original residente O(N), apertura síncrona y recorridos completos |
| `FileSession::open_in_root`, `scoped_read.rs` | Lector confinado valida handle y llena un Vec hasta `max_bytes` | Conserva garantías útiles, pero no ofrece almacenamiento lazy |
| `FileSession::external_state` | Otro `fs::read` y hash completo | Revalidar guardado puede sumar otro archivo entero en RAM |
| `FileSession::write_to_path` | `Document::to_bytes`, escritura temporal y hash del Vec | Duplicación O(N) antes de la publicación atómica |
| `jc-core/src/document.rs`, `Document` | `original:Vec<u8>`, `added:Vec<u8>` | El original completo tiene residencia/capacidad propia |
| `Document::range_chunks` / `piece_bytes` | Slices prestados de esas dos arenas; lectura infalible tras validar rango | No se puede sustituir por archivo que falla o página evictable sin cambiar el contrato |
| `reclaim_unused_storage` | Copia intervalos alcanzables del original con huecos | Una edición pequeña puede terminar reubicando muchos GB si se aplica esa ruta al backing |
| `compact_storage` / `to_bytes` / `active_bytes` | Flatten explícito | Deben quedar fuera de las rutas automáticas de GB |
| `jc-app/.../text.rs`, `TextProjection::ensure` | Primer `scan(content_start..len)` síncrono; metadatos de todas las líneas | Leer la primera pantalla recorre O(N) y puede retener O(L) |
| `LineIndex` | Bloques de hasta256 registros `u16`, base/ordinal `usize` | Soporta offsets altos en x64; sigue reteniendo un registro por línea |
| GUI/TUI apertura desde proyecto | Catálogo compartido, apertura confinada con presupuesto de bytes | El límite de16MiB del opener de proyecto es admisión actual, no arquitectura GB |
| F3/Search/Diff | Snapshots y scans limitados | Conservar sus límites; GB abierto no autoriza copiarlo entero a un proveedor |

`Document::write_to` ya recorre piezas y escribe sin flatten; es el punto útil para
un guardado streaming. Su implementación actual presupone slices residentes, por
lo que tendrá que extenderse a lectura fallible y segmentos acotados del backing.
`ContentFingerprint::of_chunks` también evita flatten, pero calcula dos hashes
síncronos por byte y no es incremental entre llamadas: puede conservarse el
algoritmo con un acumulador streaming, sin volver a reservar N bytes.

Los prefijos y posiciones de pieza son `usize`, no `u32`. En x64 no aparece un
límite estructural de4GiB en esos campos. Las reservas y sumas de nuevas longitudes
ya tienen comprobaciones importantes; todavía hay expresiones internas
`piece.start + piece.len`, subtotales y conversiones que deben verificarse con
pruebas en fronteras. Que el índice de líneas tenga tests de offsets altos no
acredita una sesión real que abra, edite y guarde un archivo de ese tamaño.

## Fuente inmutable: requisito anterior a mmap

La revisión de un documento debe nombrar bytes estables. Una vista de un archivo
que otro proceso puede escribir no es un snapshot de esa revisión. Los modos
predeterminados de `OpenOptions` en Windows comparten lectura, escritura y borrado;
no bastan para dar inmutabilidad al nuevo backing. La API oficial permite restringir
esa compartición por handle, y las restricciones permanecen hasta cerrarlo.
[Rust OpenOptionsExt](https://doc.rust-lang.org/stable/std/os/windows/fs/trait.OpenOptionsExt.html),
[CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew).

Dos políticas correctas candidatas, sin mezclarlas silenciosamente:

1. **Handle de origen protegido contra escritura.** Abrir solo lectura negando
   `FILE_SHARE_WRITE`; retenerlo durante documento/undo y comprobar identidad por
   handle, longitud y archivo efectivo. Si ya existe un escritor incompatible,
   fallar con diagnóstico. Deben verificarse separadamente los efectos de permitir
   o negar delete/rename y la publicación mediante ReplaceFile. Un lock de rango
   por sí solo no equivale a una fuente inmutable frente a todas las rutas.
2. **Snapshot privado en disco.** Copia streaming desde un origen estabilizado a
   archivo propio; memoria acotada, coste de disco y tiempo O(N) explícitos.
   Retenerlo mientras alguna pieza/undo lo use. Antes de estabilizar el origen no
   se puede prometer que la copia de un archivo cambiante represente un instante.

Primera entrega recomendada: origen protegido, sin escritores concurrentes;
lectura y pequeñas transacciones sobre piezas, undo/redo y **Guardar como** a un
destino nuevo, publicado atómicamente. El guardado sobre el mismo nombre no se
habilita hasta probar el protocolo de preservación del backing y conflictos.
La limitación debe verse en UI; no degradar esa operación a overwrite in-place.

Para el guardado completo en el mismo nombre, mantener las piezas y revisiones de
undo apuntando al original incluso después de publicar el nuevo archivo. No cerrar
el último handle y volver a abrir la ruta suponiendo que todavía nombra esos bytes.
Un snapshot privado puede evitar esa dependencia; alternativamente se debe probar
que los handles originales sobreviven a la sustitución con la compartición
seleccionada. `ReplaceFileW` documenta distintos accesos para original/reemplazo y
casos parciales1175/1176/1177: conservar la recuperación existente y ensayarlos con
el nuevo backing. No se da por compatible solo porque ambos usan Win32.
[ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew).

## Almacenamiento y lectura propuesta

El núcleo sigue siendo independiente de UI, lenguaje y rutas. Una fuente original
genérica inmutable, aportada por `jc-files`, sustituye el supuesto «siempre Vec».
Las fuentes pequeñas en memoria y las respaldadas por archivo se consumen por el
mismo contrato; las piezas siguen guardando identidad de fuente, inicio y longitud.
La arena de inserciones permanece separada, con presupuesto residente explícito.

APIs de diseño, nombres provisionales:

```rust
Document::from_source(source: ImmutableSource) -> Result<Document, StorageError>
ImmutableSource::len() -> u64
ImmutableSource::try_lease(range: Range<u64>) -> Result<ReadLease, ReadError>
ReadError::Pending(PageRequest)
ReadError::Io(...)
DocumentRead::visit_range(range, visitor) -> Result<(), ReadError>
```

`ReadLease` fija páginas inmutables y mantiene vivos sus bytes durante el préstamo.
`try_lease` no hace I/O: si faltan páginas, el owner solicita lectura posicional al
worker acotado y retorna estado pendiente. El worker recibe fuente inmutable e
intervalos, no `Workbench` ni buffer mutable. La entrega valida identidad de
fuente/generación/revisión antes de poblar la preparación; render consume solo el
frame ya preparado. Un fallo o cancelación no equivale a contenido vacío.

Comenzar con lectura posicional fallible y páginas propias es más sencillo de
integrar de forma segura que exponer un slice mmap de toda la fuente: los errores
de dispositivo se devuelven como `io::Error`. Un mmap de archivo también puede
producir `EXCEPTION_IN_PAGE_ERROR` al tocar una página; la documentación recomienda
protección SEH, que el contrato de slice Rust infalible no resuelve por sí solo.
Si después se mide mmap, usar vistas de solo lectura por ventanas y RAII, offsets
alineados a la granularidad obtenida del sistema, no una constante supuesta.
[MapViewOfFile](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-mapviewoffile).

No usar `FILE_MAP_COPY` para «resolver» edición de varios GB: Microsoft documenta
que su commit se cobra por la vista completa. Editar debe agregar piezas de
inserción y conservar rangos del original, sin volver escribible la fuente.
[Acceso copy-on-write](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-mapviewoffile#parameters).

Cuotas iniciales candidatas, pendientes de medir: páginas64KiB, caché total8MiB
por sesión GB, como máximo dos lecturas en vuelo y una intención pendiente por
vista; prefetched visible primero, escaneo opcional a baja prioridad cooperativa.
Contabilizar páginas fijadas por leases/snapshots además de las evictables. No
admitir páginas sin cuota, ni crear un thread por página. El presupuesto1MiB de
layout existente es distinto de la nueva caché de lectura y debe seguir explícito.

## Índice textual progresivo

La primera pantalla no debe pedir un catálogo de todas las líneas. Propuesta:
resumen por intervalo de bytes y catálogos locales evictables. Cada resumen guarda
rango fuente, estado conocido/desconocido, cantidad de EOL y contexto CRLF en sus
bordes. Los índices escalares/grafemas y su cuota siguen siendo locales al rango.
Un escaneo cancelable puede completar resúmenes en tramos; los eventos no hacen
un loop hasta EOF. Cambiar revisión invalida solo los intervalos afectados y los
bordes, sin desplazar un vector global por cada línea posterior.

Acceso por byte y lectura relativa del viewport se habilitan antes del conteo
global. «Ir a línea N» y números absolutos lejanos esperan que el prefijo relevante
esté indexado, o muestran progreso cancelable; no inventar un número ni bloquear
teclado. El final del archivo puede abrirse por byte, con conteo de línea pendiente.
El EOL dominante global también está pendiente mientras falten resúmenes: Enter
usa una política explícita/local, sin fingir un conteo completo.

Ejemplos aritméticos:4GiB de `abcdefg\n` producen536870912 líneas más la final;
solo dos bytes de registro por línea ya rondan1GiB, sin encabezados/capacidades.
Con un LF por byte, el mínimo de registros ronda8GiB. En contraste, un resumen por
MiB requiere4096 resúmenes para4GiB, además del catálogo local acotado. El tamaño
exacto de cada resumen y los tiempos deben medirse; no se atribuye esta estimación
al binario actual ni se promete RAM constante para cualquier cantidad de piezas.

Una sola línea de varios GB no debe crear un layout completo: continuar checkpoints
por rangos y hacer explícito cualquier límite de navegación/cluster. Un destino de
bytes no autoriza caret dentro de UTF-8/CRLF; comprobar contexto suficiente con las
conversiones existentes y rechazar una operación textual que exceda su presupuesto.

## Edición, historial y guardado

Una inserción pequeña en offset>4GiB crea referencias antes/después al backing y
una pieza añadida. Conserva revisión esperada, validación de solapamientos,
reservas previas y publicación atómica de una transacción, con su mapa de posiciones.
Undo/redo retienen referencias al backing, no clones del archivo. El vector actual
de piezas puede seguir inicialmente: su coste depende de P, no N; cambiarlo por
árbol exige medir muchas ediciones, no se justifica solo por el tamaño del archivo.

La contabilidad actual cobra bytes eliminados+insertados en el historial lógico;
una eliminación de GB superaría sus8MiB aunque el payload original no se copie.
Primera entrega puede conservar ese límite con información explícita de undo
disponible. Para prometer undo de eliminaciones enormes hay que separar cargo
residente de inserciones/metadatos y retención del backing en disco: es una decisión
real de historial, no llamar «gratis» a todo rango del archivo original.

El collector no copiará el original respaldado por archivo para eliminar huecos.
Se descartan referencias o fuentes sin piezas alcanzables; solo la arena añadida
se compacta bajo su cuota. `compact_storage` queda explícito y presupuestado.

Guardar escribe piezas en segmentos acotados al temporal, calcula fingerprint
streaming, permite cancelar antes de publicar y sincroniza antes del reemplazo.
Antes de commit se revalida la intención de documento/revisión/ruta y la versión
de disco. Longitud/mtime aislados no son equivalentes al fingerprint actual; la
identidad+handle protegido puede aportar la garantía mientras permanezca vigente,
y el resto requiere comparación streaming o snapshot estable. Guardar varios GB
seguirá siendo O(N) en I/O y necesitará espacio libre para el temporal; debe ocurrir
como tarea versionada, sin mutar el modelo desde el worker ni bloquear el teclado.

## Bloques y pruebas de aceptación propuestos

1. Fuente inmutable y sesión GB64bit, lectura posicional acotada, metadatos/errores,
   leases y cuotas; reutilizar el modelo único de documento. Casos de fuente vacía,
   fragmentos cruzando4GiB, EOF, read parcial/error y cancelación.
2. Proyección progresiva y preparación visible; sin scan/hash O(N) antes de la
   primera pantalla. LF/CR/CRLF entre páginas, BOM/encoding explícito, inválidos,
   línea extrema, salto a byte/final e índice de línea aún pendiente.
3. Transacciones pequeñas antes/en/después de4GiB, varias vistas, undo/redo,
   recolección sin materializar original y SaveAs atómico nuevo. Comparar fuente
   por hash streaming, nunca cargar el fixture de varios GB en Vec de test.
4. Guardado mismo nombre con backing retenido, conflicto externo/rename/truncado,
   escritor ya abierto, disco lleno/cancel/error y recuperación1175/1176/1177.

Corpus propio:1GiB,4GiB+64KiB y8GiB en máquina x64, casos densos y línea extrema.
Archivos sparse sirven para offsets y errores, pero no acreditan rendimiento de
lectura fría de un archivo físicamente asignado; mantener ambas categorías separadas.
No reservar esos corpus ni llenar el disco durante esta admisión.

Medir apertura hasta primera pantalla, bytes realmente leídos antes de ella,
primer salto frío/remoto frente a repeticiones calientes, edición/undo p50/p95,
cancelación, guardado streaming, bytes mapeados/commit/working set/private bytes,
páginas retenidas y metadatos de líneas/piezas. No invalidar la caché global de
Windows ni tocar procesos/servicios ajenos para simular frío. Registrar latencia
física como abierta si solo se observan modelo o bitmap/ConPTY.

Nada de lo anterior certifica soporte GB actual. Esta admisión identifica la
migración requerida; subir límites sin esas rutas no la implementa.

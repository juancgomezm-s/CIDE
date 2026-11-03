# Search Plus de proyecto — literal, lotes y reemplazo versionado

**CPU actualizado:** un coordinador y helpers persistentes por archivo, default dos trabajadores totales y cuotas globales; [diseño](CPU_PROJECT_EXECUTOR.md), [datos/límites](CPU_PERFORMANCE.md) y [opción TUI](CPU_RUNTIME_OPTIONS.md). La implementación de un solo worker descrita en el checkpoint original es histórica.

**Seguimiento S0:** el matcher de bytes descrito en este checkpoint fue sustituido por un engine compartido de fragmentos Unicode; [estado y mediciones actuales](S0_DOCUMENT_SEARCH.md). S1 mantiene su interfaz exacta sensible a mayúsculas y sus cuotas. El resto documenta el checkpoint S1 y su evidencia histórica.

Implementado localmente el 2026-11-02 sobre `ed51143`, después de F1/F2/F3.
Núcleo transversal en jc-app; TUI consume el mismo controlador y las transacciones
existentes. No se añadió dependencia de terceros, indexador residente ni servicio
de lenguaje. ecode sigue como referencia de flujo; no se portó su código.

## Uso actual

1. Abrir una carpeta desde File / Open Folder.
2. Ctrl+Shift+F, texto literal, Enter; indicar globs de inclusión separados por
   coma (vacío = todos), Enter; exclusiones adicionales, Enter. Por defecto se
   excluyen `.git/**`, `target/**` y `work/**`.
3. Resultados llegan sin otra tecla. Up/Down/Home/End y Enter navegan desde el
   panel; Esc en el panel inferior o Search / Cancel Project Search cancelan.
4. Tras una búsqueda completa sin avisos ni fuentes obsoletas, Search / Replace
   in Project solicita el reemplazo (vacío elimina coincidencias).
5. Revisar el preview agrupado por archivo: ubicación, rango de bytes, contexto y
   pares `-`/`+` de las sustituciones. Son extractos, no un diff completo de archivo.
   F9 o Ctrl+Enter confirman; Esc cancela sin editar.
6. Output informa cada resultado/conflicto. Se procesa un archivo por turno y Esc
   detiene los restantes. Los abiertos se editan sin guardar; los cerrados se
   guardan con FileSession. Un undo revierte el lote de un documento abierto.

**Alcance textual:** coincidencia literal exacta, sensible a mayúsculas, Unicode
y legacy explícito. No se ofrecen switches ficticios de regex o case folding
completo. La búsqueda de documento S0 conserva sus límites históricos (incluido
case-insensitive ASCII); su migración acotada y case folding real siguen pendientes.
La code page legacy real del usuario continúa sin confirmarse. En TUI el encoding
de proyecto procede de la opción de arranque `--encoding`; las sesiones abiertas
conservan su propio encoding, que tiene prioridad. Sin configuración solo se
reconoce UTF-8 válido/BOM/ASCII; legacy ambiguo produce un aviso.

## Propiedad y trabajo acotado

`ProjectSearch` mantiene generación, raíz, estado y filas tipadas. Una petición
captura bytes propios de documentos abiertos incluidos en los filtros, con
DocumentId/revisión/encoding; no clona Document, FileSession, vistas ni undo.
Los archivos nuevos abiertos también participan. Se escanean esos snapshots
primero y se evita su duplicación desde disco, incluso cuando hay texto sucio.

Hay un worker de proyecto, creado bajo demanda y reutilizado. Solo conserva una
petición pendiente; una consulta nueva cancela/sustituye la anterior. Comparte
Wakeup y Cancellation con F3, con colas específicas para lotes y contexto de
proyecto (el proveedor F3 original está ligado a documentos). Si además se usan
los jobs internos F3, la TUI admite por defecto hasta tres workers en total:
dos de JobSystem y uno de proyecto. No se lanza un pool por consulta.

| Recurso | Cuota |
|---|---|
| Consulta / reemplazo | 4096 bytes UTF-8 cada uno |
| Globs | Hasta 16 en total, 128 bytes cada uno; `*`, `?`, `**`, sin regex |
| Documentos capturados | 128 incluidos; 2 MiB/archivo, 8 MiB/petición |
| Snapshots simultáneos | Un trabajo activo y uno en captura/pendiente: hasta 16 MiB de bytes |
| Archivo de disco | Lectura acotada a 2 MiB; no se lee todo un archivo que exceda la cuota |
| Consulta completa | 64 MiB examinados por defecto; máximo configurable 256 MiB |
| Recorrido | 100.000 entradas y profundidad 32 como máximo |
| Trabajo | Deadline cooperativo de 10 s; consulta de token cada 4096 bytes y entre archivos |
| Resultados | 1000 filas por defecto (avisos incluidos), máximo configurable 10.000; 2 MiB de carga lógica |
| Entrega | Lotes de hasta 32 filas, flush al terminar cada archivo; cuatro lotes en cola |
| Fin/cancelación | Puede existir un quinto evento terminal; no lleva otra colección de resultados |
| UI | Hasta cuatro eventos por vuelta; filas visibles únicamente en render |

Las cuotas de filas/capacidades ocupadas no son un límite global de memoria privada.
El texto se busca con KMP sobre bytes codificados estrictamente y una tabla por
encoding/consulta, reutilizada entre archivos. Los índices para posición se
decodifican por fragmentos de hasta 16 KiB mientras avanza la búsqueda; cada snippet
lee hasta 160 bytes y muestra hasta 80 escalares. No se decodifica todo el archivo
por resultado ni se reconstruye el índice del editor al dibujar.

La cancelación libera la cola pendiente y despierta al productor bloqueado. El
loop principal comprueba backlog de proyecto antes de dormir; completion no
depende de otra tecla. Un fallo capturado conserva la señal de recuperación de
terminal aunque se haya cancelado esa generación. I/O del sistema puede bloquear:
el token no interrumpe una llamada arbitraria al filesystem ni garantiza un tiempo
de cancelación universal en volúmenes remotos/cloud.

## Identidad, coordenadas y raíz

Cada match conserva ruta, rango absoluto de bytes, posición de escalares cero-based,
encoding, snippet y origen: TaskStamp de documento abierto o ContentFingerprint
de bytes leídos. No confundir sus columnas con grafemas, ancho visual o LSP UTF-16.
CRLF no admite coincidencias que empiecen/terminen en su interior; BOM inicial no
se busca como contenido. No se normalizan bytes, EOL ni newline final.

La entrega valida generación y revisión/encoding/vida de los abiertos. Después de
editar se retiran filas obsoletas y se marca el conjunto como stale para impedir
reemplazo parcial accidental. Navegar vuelve a comprobar versión; si el archivo
cambió, se pide repetir la consulta. No se lee disco por cada tecla para simular
un watcher. Los resultados de disco son snapshots hasta esa verificación explícita.

El recorrido excluye symlinks/reparse points y verifica raíz canónica. En Windows
se comprueba además el nombre final del handle abierto antes de leer; Linux usa
/proc/self/fd (sin validación local Linux en este incremento). Otras plataformas
no habilitan lectura segura de proyecto todavía. NUL, UTF-8 inválido, encoding
ambiguo, archivos demasiado grandes y errores de lectura producen avisos; no
se ocultan como cero coincidencias. La política binaria inicial es NUL/encoding,
no un detector universal de formatos.

## Reemplazo explícito

`ProjectReplacePreview` es un modelo de lectura sobre la consulta ya realizada;
crearlo no abre ni escribe archivos. Su texto se agrupa por archivo y muestra
sustituciones con contexto acotado. No certifica que disco permanezca igual mientras
el usuario revisa: cada archivo se comprueba otra vez al aplicar.

Solo se habilita después de un conjunto completo sin avisos/obsolescencia. Se
valida encoding estricto y presupuesto antes de aceptar: propuestas hasta 2 MiB
por archivo, entrada/salida total de hasta 8 MiB cada una, salida de hasta 2 MiB
por archivo y texto de preview hasta 2 MiB. Un carácter no representable rechaza
el preview entero. El preview queda ligado a generación y raíz.

La aplicación consume un plan por turno, nunca desde un worker. Para abiertos usa
`apply_text_edits` con revisión esperada y un único undo; conserva vistas y no
guarda automáticamente. Para cerrados comprueba huella y que no se hayan abierto
desde la consulta, prepara un lote en FileSession y guarda con detección de
conflictos y el tratamiento Windows de `ed51143`. Los reemplazos sin efecto no
crean undo ni reescriben disco. Cambios posteriores de revisión, encoding, raíz,
contenido o propiedad producen conflictos individuales, sin sobrescritura forzada.

Se informa éxito/conflicto por archivo. Cancelar conserva lo ya completado y deja
los planes restantes sin aplicar. **No hay transacción ni undo global del filesystem**.
Una operación de archivo ya iniciada termina antes de atender la cancelación;
puede bloquear el hilo de aplicación durante su I/O/esperas acotadas de guardado.

La comprobación de versión no es un CAS del filesystem. El commit hereda las
limitaciones de nombres/races de FileSession; no se acredita aislamiento frente
a un actor que cambie directorios precisamente durante el commit. Sí se probaron
junction fuera de raíz y redirección después del preview sin modificar el destino
externo. Hardening adversarial y matriz multiplataforma siguen abiertos.

## Validación y mediciones

Entorno del equipo: Windows 11 Pro 10.0.26300, Ryzen 9 8945HS (8/16), RAM visible
16.037.152 KiB, Rust 1.85.1 MSVC; dependencias offline, release de tamaño existente.
Comprobaciones y conteo final en VALIDACION. No se ejecutó CI remoto.

Regresiones: globs/recursión/avisos, BOM/CRLF/Unicode y tres encodings legacy,
snapshots sucios, generaciones, cancelación bajo presión, límites, root/handle,
navegación obsoleta, preview sin escrituras, encoding estricto, conflictos,
reemplazo de abiertos en un undo, archivo abierto tras búsqueda, no-op y junction
creada después del preview. La TUI prueba prompts, cancelación, entrega sin entrada,
preview/confirmación y un archivo por vuelta con cancelación de los restantes.

`project_search_bench` mide un corpus ASCII de **2.097.144 bytes en cuatro archivos**,
64 matches, cinco consultas de calentamiento +31 retenidas, percentiles ordenados
en índices 15/29, cache de disco caliente. Primera entrega p50/p95 5707/7220 µs;
fin completo 21239/27457 µs. Cancelar y hacer join después de la primera entrega,
con otra consulta densa: 56/73 µs. No incluye terminal ni reemplazo de disco.
[S1-search.csv](benchmarks/S1-search.csv).

`scripts/smoke-conpty.ps1 -ProjectSearch` comprueba el ejecutable real: abrir carpeta,
consulta/filtros por menús, entrega sin otra tecla, navegación, preview cancelado,
aplicación confirmada y un undo; el buffer abierto no se guarda. El host propio
configura **su ConPTY privada** en UTF-8 e interpreta movimientos relativos y
borrado VT; no cambia ajustes del sistema ni consola del usuario.
[S1-conpty.json](benchmarks/S1-conpty.json).

El control de cursor previo al recorrido registró p50/p95 15,6115/16,0598 ms hasta
salida VT, 0 clears por teclas, un clear por resize y exit 0. Muestreo del proceso
mínimo: 1.142.784 bytes privados, 0 ms CPU durante 250 ms. No es medida de RAM del
proyecto con servicios, ni de pantalla física, ni cierra las metas de <50 MiB o
p95 <16 ms. No se validan GUI, DPI/IME, ancho por grafemas, AVM ni Linux.

Sigue abierto el cierre completo de Search Plus/M2A: búsqueda de documento acotada
con Unicode case folding real, regex con motor/presupuesto reales, mayor matriz
de corpus/filtros/teclado y rendimiento global. La GUI conserva dirección visual
Sublime Text; Progress real M3, CI/Git con escritura M5 y plugins externos post-M5.

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

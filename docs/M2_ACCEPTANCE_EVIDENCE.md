# M2 — Corpus ampliado, arranque y memoria por proceso

Bloque del 2026-11-02 sobre `282d006`, Windows 11 Pro 10.0.26300, Ryzen 9 8945HS
8/16, Rust 1.85.1 MSVC offline. Perfil existente `opt-level="s"`, thin LTO, un
codegen unit, sin nuevas dependencias. Datos crudos conservados antes/después.
No constituye aceptación completa de M2A ni comparación con otro editor.

## Hallazgo corregido

El layout frío de una línea materializaba todos sus caracteres y offsets antes
de construir checkpoints dispersos. Una línea ASCII de 16 MiB hizo alcanzar al
editor real **456.040.448 bytes de compromiso máximo (434,9 MiB)**. Su memoria
después de arrancar era solo unos 18 MiB: medir únicamente el estado final ocultaba
el pico. El caso UTF-8 del modelo instrumentado también lo reprodujo.

`TextProjection::layout` ahora recorre rangos prestados de 64 KiB, con cuatro bytes
de contexto, mediante el decoder único `DecodedScalars`. Solo retiene checkpoints
dispersos y el conteo de escalares; no construye el String/vector de offsets de
toda la línea. El buffer se reutiliza durante esa construcción y se libera al
terminar. Se conserva el presupuesto de layout de 1 MiB/documento, índices por
revisión/encoding, agrupación inválida y BOM interior. No cambia fuente/undo/save.

En el ejecutable real, el mismo caso quedó en **18.833.408 bytes (18,0 MiB)** de
compromiso máximo. En el modelo UTF-8, el primer layout pasó de 100,04 a 73,77 ms
y de 117.444.841 a 17.371.537 bytes vivos máximos solicitados al allocator.
Es una muestra fría por corpus, no un percentil ni una garantía de latencia.
Sigue siendo necesario recorrer la línea inicialmente; la operación fría no se
vuelve incremental entre turnos de UI en este bloque.

## Protocolo del modelo

`jc-app/examples/m2_acceptance.rs`, con allocator de medición exclusivamente en
el ejemplo (`examples/support/mod.rs`). Nunca se enlaza al editor. El wrapper
cuenta alloc/alloc_zeroed/realloc, suma tamaños solicitados y registra live/peak
por fase. No representa overhead/alineación internos del heap ni simultaneidad
transitoria dentro de System.realloc. Los contadores añaden coste: estos tiempos
no deben compararse como si fueran los benchmarks S0 sin instrumentación.

Un proceso nuevo por corpus, fixtures exclusivos bajo TEMP del proyecto:

| Corpus | Fuente | Geometría |
|---|---|---|
| ascii | 4.194.304 bytes UTF-8 | Líneas de 8 bytes |
| unicode | 4.194.294 bytes UTF-8 | é, emoji, combining, CJK, Σ/ς, İ, ﬃ y CRLF |
| cp1252 / latin1 / ibm850 | 4.194.291 bytes por página explícita | niño/Straße/été y CRLF |
| long | 16.777.212 bytes UTF-8 | Una línea multibyte sin newline final |
| pieces | 4.194.304 + 2048 bytes | Un lote de 2048 inserciones separadas: 4097 piezas |
| multi | Ocho fuentes de 4 MiB, 32 MiB total | Líneas de 8 bytes; índices de los ocho visitados |

Se separan abrir, primer índice/layout, crear dos vistas, movimiento/consulta/
viewport de ambos grupos, búsqueda ausente, editar/undo/redo/undo, liberar historial,
cerrar todos y destruir Workbench. Las operaciones calientes usan cinco
calentamientos y **31 muestras individuales retenidas**, sin promedios de lotes.
Los grupos de multi muestran documentos diferentes; los demás comparten documento.
Las estadísticas de fuente/proyección de cada fila son del documento activo,
mientras los contadores de allocator/proceso cubren todo el proceso del ejemplo.
En pieces, la fase fragment ya prepara índices/layout al reubicar posiciones:
su fila cold_projection posterior está caliente y no mide un primer acceso frío.
Edit/undo/redo/undo incluye reprogramar una consulta existente, sin completar ese
scan entre ediciones; no se debe interpretar como búsqueda completa tras cada tecla.

Private/working set/peak commit vienen de GetProcessMemoryInfo. El pico del SO es
acumulado durante la vida del proceso, incluyendo preparar el fixture; el peak de
allocator se reinicia por fase. Cerrar y destruir no obligan al heap/Windows a
devolver toda reserva. No se vació la caché del sistema ni se aisló toda la carga.

Resultados calientes finales, p95 en µs:

| Corpus | Right + posición + viewport de dos vistas | Búsqueda ausente completa | Edit/undo/redo/undo |
|---|---:|---:|---:|
| ascii 4 MiB | 62,0 | 40960,4 | 942,7 |
| unicode 4 MiB | 81,8 | 55418,4 | 301,4 |
| Windows-1252 | 74,4 | 64851,9 | 453,3 |
| ISO-8859-1 | 74,0 | 61808,1 | 445,8 |
| IBM850 | 74,9 | 64102,4 | 453,9 |
| long 16 MiB | 53,0 | 528241,9 | 172,2 |
| 4097 piezas | 58,6 | 39707,6 | 981,3 |
| ocho documentos | 53,8 | 39861,2 | 985,4 |

El fin de búsqueda no es el tiempo de un turno ni el de una tecla. La dispersión
de la línea larga es alta. Las cuotas y la cancelación de búsqueda no se cambiaron.
El layout frío corto no mejoró consistentemente: ASCII 9,57 → 10,54 ms y Unicode
8,26 → 8,72 ms en esas muestras únicas; no se afirma mejora general de throughput.

La búsqueda ausente UTF-8/legacy usa siete asignaciones p95/~16,7 KiB solicitados
por consulta de 4 MiB, no por fragmento. Los límites de memoria de cada subsistema
no son un techo de RAM global. Con ocho documentos de líneas de ocho bytes, el
proceso llega a **72.847.360 bytes privados (69,5 MiB)** al preparar los índices:
32 MiB de fuentes más metadatos de líneas y estado. Tras cerrar todos queda en
1.216.512 bytes privados en esa corrida. Sigue abierta la representación compacta
de metadatos de líneas; no se oculta la carga que excede 50 MiB.

En la línea larga, liberar historial puede recolocar fuentes conservadas: el pico
de compromiso del modelo final llega después a **35.049.472 bytes (33,4 MiB)**.
La corrección del layout no elimina los picos de otras fases. El cache disperso
frío reserva hasta ~0,5 MiB; el caso Unicode anterior retenía ~0,2 MiB, ambos dentro
de la cuota. No se interpreta quitar un pico como reducir toda memoria retenida.

[Antes crudo](benchmarks/M2-evidence-before-model.csv),
[después crudo](benchmarks/M2-evidence-after-model.csv),
[resumen antes](benchmarks/M2-evidence-before-model-summary.csv),
[resumen después](benchmarks/M2-evidence-after-model-summary.csv).

## Protocolo nativo y procesos auxiliares

`scripts/conpty-acceptance.cs` amplía el host privado existente. Crea exclusivamente
sus fixtures, editor, host Rust y ConPTY oculta; no toca sesiones de usuario.
Identifica el conhost hijo de su propio proceso mediante Toolhelp, sin consultar
contenido de otros procesos. Muestrea memoria solicitando intervalos de 5 ms;
la granularidad efectiva depende del planificador. El CSV registra timestamps,
PIDs y contadores de cada proceso. El máximo de una suma usa lecturas del mismo
ciclo de muestreo, no suma picos individuales de momentos distintos; las lecturas
del ciclo tampoco son atómicas.

Cada forma ejecuta **ocho procesos nuevos: uno de calentamiento y siete retenidos**.
Arranque se mide desde CreateProcess del host hasta primer frame y respuesta a
Right (`Col 2`), incluyendo esa entrega al terminal. No incluye crear ConPTY ni
preparar fixtures, ni prueba archivo caché-fría. p95 de siete arranques es el máximo
observado, con evidencia estadística limitada. Cada proceso tiene cinco Right de
calentamiento y 31 retenidos, agrupados aquí en 217 muestras por forma.

Ventana 140×40, Split Right desde View, movimiento con dos vistas, visita por
Alt+1..8, One Group, Ctrl+W de todos y File/Exit. Formas: base 4 KiB; multi ocho
fuentes ASCII de 4 MiB con líneas de 64 bytes; long una línea ASCII de 16 MiB.
El corpus nativo multi tiene menos líneas que el modelo multi: sus memorias no
son comparables como si fueran la misma carga. Todos los procesos salieron con 0.

| Forma | Arranque antes p50/p95 ms | Arranque después p50/p95 ms | Right hasta VT después p50/p95 ms |
|---|---:|---:|---:|
| base | 54,33 / 58,34 | 59,12 / 63,95 | 15,872 / 16,084 |
| ocho documentos | 114,69 / 123,31 | 99,82 / 114,44 | 15,606 / 16,160 |
| línea 16 MiB | 234,68 / 244,85 | 190,64 / 203,64 | 15,978 / 16,048 |

Memoria final: máximos observados entre las siete corridas, MiB = 1.048.576 bytes.
Peak commit es el máximo acumulado que informa Windows, no una muestra de private.

| Forma | Editor peak commit antes → después MiB | Host private máximo MiB | ConPTY private máximo MiB | Suma private editor+host+ConPTY máxima MiB |
|---|---:|---:|---:|---:|
| base | 1,38 → 1,37 | 0,71 | 1,57 | 3,61 |
| ocho documentos | 38,86 → 38,91 | 0,72 | 1,57 | 41,16 |
| línea 16 MiB | 434,91 → 17,96 | 0,72 | 1,56 | 20,24 |

El controlador C# de medición consume hasta ~23,5 MiB privados después y se
registra aparte; no es un componente del editor. El host Rust también es un
auxiliar del test para abrir CONIN$/CONOUT$, no un servicio de producto. ConPTY
representa este transporte; una GUI o terminal visible tendría otros procesos.
Progress/LSP no se iniciaron y no se les atribuye consumo cero bajo carga.
Después de cerrar, editor private máximo observado: 1,34 / 2,41 / 1,32 MiB según
forma. Las muestras no excluyen toda retención posterior ni prueban ausencia de fugas.

[Tiempos antes](benchmarks/M2-evidence-before-native-timings.csv),
[tiempos después](benchmarks/M2-evidence-after-native-timings.csv),
[memoria antes](benchmarks/M2-evidence-before-native-memory.csv),
[memoria después](benchmarks/M2-evidence-after-native-memory.csv).

## Matriz funcional y controles de cierre

**194 pruebas del workspace, 0 fallos**; fmt --check, Clippy workspace/all-targets
-D warnings, release del workspace y ejemplo instrumentado. Se construyó el bloque
antes de ejecutar la suite. No se repiten suites por cambios documentales.

- Corpus integrado de ocho bloques por encoding: UTF-8/Windows-1252/Latin1/IBM850,
  Unicode multibyte/combining, EOL mixtos, posiciones contra TextIndex independiente,
  rangos originales, fragmentación/undo y replace-all con una transacción sin guardar.
- Layout frío: escalar multibyte sobre cada borde próximo a 64 KiB, BOM interior,
  grupo inválido y truncado al EOF, posiciones y conversión inversa.
- Teclado TUI: Ctrl+D, Ctrl+Shift+D, Tab y Ctrl+/ en las cuatro páginas con un undo;
  Tab/BackTab en Explorer, Output, Find, Replace, palette y menú no editan fuente.
- Las regresiones previas de vistas, selección, búsquedas, archivos y colas pasan.
- El recorrido Unicode del ejecutable real volvió a aprobar STRASSE/Straße sin
  otra tecla, F3, Ctrl+G escalar, reemplazo sin guardar y undo:
  [resultado](benchmarks/M2-evidence-document-conpty.json).

No se afirma revisión manual exhaustiva de teclado, portapapeles del sistema,
IME/grafemas/UAX #29, accesibilidad, DPI, Linux, GUI ni OpenEdge real.

## Reproducción y pendientes

Compilar con el toolchain/perfil local, sin red. `m2_acceptance` contiene el allocator
instrumentado y no debe confundirse con `jc-ide-tui` sin ese wrapper.

```text
cargo build -p jc-app --example m2_acceptance --release --locked --offline
python -X utf8 scripts/measure-m2-model.py before
cargo build --workspace --release --locked --offline
cargo build -p jc-tui --example f3_console_host --release --locked --offline
csc /nologo /codepage:65001 /out:work\m2-native.exe scripts\conpty-smoke.cs scripts\conpty-git.cs scripts\conpty-acceptance.cs
work\m2-native.exe . before
```

Repetir los mismos comandos de medida con `after` tras compilar la corrección;
`python -X utf8 scripts/summarize-m2-evidence.py` genera resúmenes desde los datos
crudos. Los runners rechazan sobrescribir mediciones existentes: usar una copia
de trabajo de evidencia para una nueva corrida. Para medir el antes se necesita
el código productivo de `282d006` y los mismos ejemplos/scripts de este bloque;
no reconstruirlo cambiando el árbol actual con modificaciones ajenas.

**Pendientes de aceptación:** metadatos O(L) en muchos documentos de líneas muy
cortas, arranque realmente caché-frío/varias máquinas, distribuciones de operaciones
frías, picos durante recolección/ediciones extremas y mayor matriz de teclado.
Right hasta VT p95 sigue alrededor de 16 ms y a veces lo supera: no acredita
<16 ms hasta pantalla física. No hay una medición óptica/presentación física con
estos medios; ese pendiente se conserva sin bloquear las mediciones independientes.
El objetivo global <50 MiB no se acepta por una muestra pequeña que lo cumple
cuando otra carga lo excede. Sin CI remoto, push, instalaciones ni cambios de
seguridad/configuración, servicios, Qwen o GPU.

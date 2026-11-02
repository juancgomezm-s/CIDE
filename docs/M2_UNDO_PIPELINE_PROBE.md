# M2-F — Separar undo dentro del proceso del recorrido ConPTY

Estado: compilado y ejecutado localmente, en dos pares con orden invertido y
una repetición nativa por versión, sin builds/fixtures concurrentes.

## Pregunta y frontera

Un tiempo observado entre inyección de tecla y VT puede incluir entrega del
input, planificación del proceso, manejo del evento, preparación/composición,
codificación, transporte y observación del harness. La mejora de undo en el
modelo de metadatos no implica que todos esos componentes mejoren a la vez.

`crates/jc-tui/examples/undo_pipeline.rs` separa tres tramos del frontend real,
sin modificar producto ni añadir accesores: `TuiApp::handle_event`,
`TuiApp::frame` y `FrameEncoder::draw_to` sobre un `Vec<u8>`. Utiliza solo las
APIs públicas que ya usa `f3_pipeline`. Todos los cambios pasan por eventos y
comandos centrales; no accede a Workbench, documentos, proyección o historial
privados, no clona buffers y no incorpora otro motor de edición.

Los tiempos son **wall time dentro del proceso**, obtenidos con `Instant`.
Caracterizan trabajo de la pipeline ejecutado localmente, pero incluyen posibles
preempciones. No son contadores de tiempo de CPU del proceso/hilo. No incluyen
lectura de eventos del SO, ConPTY, pipes, el host externo, dibujo de consola ni
presentación física. Tampoco son una medición de RAM. Sirven para comparar
etapas bajo dos ejecutables antes de atribuir la variación nativa al núcleo.

## Corpus y escena

El ejemplo se ejecuta desde una raíz de proyecto que contenga `Cargo.toml` y
`crates/jc-tui/src/lib.rs`. Crea exclusivamente un directorio nuevo
`work/test-temp/undo-pipeline-PID`, rechazando uno existente, y ocho fuentes
`document-0.txt`…`document-7.txt` con `create_new`. Usa rutas relativas en
`LaunchOptions` para mantener visible la posición en la barra de estado.
La misma fuente del ejemplo puede enlazarse con la versión plana conservada y
con la implementación actual; no depende de APIs añadidas para la estructura.

Cada fuente mide exactamente **4 MiB** y reproduce el corpus de
`conpty-acceptance.cs` en modo `dense-edits`: `M2_VISIBLE_n `, 128 `x`, LF y
repeticiones de `abcdefg\n` hasta el tamaño final, truncando el último fragmento
si hace falta. Se genera mediante un bloque de 64 KiB y contador `remaining`,
sin consultar longitud de archivo por cada línea. La apertura declara UTF-8;
no se decodifican ni normalizan los fuentes para comprobar sus bytes.

La geometría medida es siempre **140×40**. Alt+Shift+2 crea Split Right y Ctrl+1
enfoca el grupo primario mediante comandos centrales. El grupo secundario
permanece sobre `document-7.txt`. Se visitan los ocho documentos con Alt+1…8.
En cada selección, Ctrl+G, `Event::Paste("1:1")` y Enter confirman el origen
fuera del timer. No se mide el Enter que cierra ese prompt.

## Series medidas y controles

Para cada documento se ejecutan cinco parejas Enter/undo de calentamiento y
31 parejas conservadas. Cada Enter inserta una nueva línea al inicio; cada
Ctrl+Z devuelve el documento a su estado limpio antes de la pareja siguiente.
No se reconstruye la aplicación ni se fuerza recolección de memoria entre
muestras. Los ocho documentos permanecen vivos durante el recorrido.

Cada operación tiene cuatro campos de tiempo:

| campo | trabajo incluido |
| --- | --- |
| `handle_event_ns` | un evento Enter o Ctrl+Z por la ruta pública de la TUI |
| `frame_ns` | `frame(140,40)`: preparación de viewport/presentación y composición |
| `encode_ns` | `FrameEncoder::draw_to` y escritura/flush al Vec en memoria |
| `total_ns` | recorrido de esos tres tramos más el bookkeeping de sus relojes |

La creación del evento y `sink.clear()` quedan fuera del tiempo total. El Vec
de salida se reutiliza; su capacidad se calienta al dibujar la escena inicial.
La codificación sí incluye sus asignaciones internas, comparación de celdas y
copia del frame reconocido que realice `draw_to`. Los controles, creación de
fixtures, apertura, navegación previa, lectura de disco y salida CSV quedan
fuera de los cuatro timers. No se resta una estimación de overhead de reloj.

Tras cada operación se comprueban dimensiones, encabezado de grupo primario
activo y documento correcto, grupo secundario, y barra de posición: `Ln 2 Col 1`
después de Enter y `Ln 1 Col 1` después de undo. También se exige que no quede
un prompt de posición ni diálogo de cambios pendientes, que los bytes
codificados coincidan con la longitud del Vec y que no haya repaint completo
(`full=false`) en las operaciones estables.

A 140 columnas la última pestaña puede quedar recortada. Para comprobar **cada
documento** y su indicador `*`, se compone un frame de verificación de 300×40
fuera de los timers. Deben verse la pestaña activa con estilo Reverse, su
indicador después de Enter, y ningún `*` después de undo. No se codifica esa
composición como muestra. Se restaura 140×40 y su baseline reconocido por el
encoder antes de la siguiente operación medida.

**Esta verificación añade preparación y calentamiento de layouts/cachés.**
El probe permite una comparación entre builds con el mismo trabajo adicional;
no reproduce exactamente la secuencia ni los tiempos del harness ConPTY. Sus
datos no deben restarse directamente de los p95 nativos para fabricar una
latencia residual: son recorridos distintos y percentiles no aditivos.

Al terminar cada documento se verifican por streaming todos sus bytes de disco
contra el patrón original, incluidos tamaño, EOL y fragmento final. También
se verifican los ocho antes y después del recorrido. One Group y ocho Ctrl+W
deben dejar `Group 1: Empty` sin responder a ningún diálogo de cambios. File /
Exit debe producir `should_exit() == true`. Es una comprobación de la intención
de salida limpia dentro de la app, no una validación adicional del cierre del
proceso/host ConPTY. La limpieza elimina solo los ocho archivos propios y el
directorio vacío, sin borrar árboles recursivamente.

## Datos crudos y uso

El ejemplo acumula las filas en memoria y emite CSV únicamente después de
superar todos los controles. No escribe CSV entre parejas medidas:

```text
document,operation,sample,handle_event_ns,frame_ns,encode_ns,total_ns,cells,runs,bytes,full
```

`document` es 0…7, `operation` es `enter` o `undo`, `sample` es 0…30. Se esperan
**496 filas de datos**: ocho documentos × dos operaciones × 31 muestras. Las
80 operaciones de calentamiento se descartan. `cells`, `runs`, `bytes` y `full`
proceden del encoder de la operación medida, no del frame de verificación.
Cada fila representa una operación individual, sin lotes ni división de tiempos.

La ventana serial utilizó el mismo ejemplo con ambos códigos, conservando
identidades y hashes de ejecutables y destinos nuevos sin sobrescribir evidencia:

```text
cargo build -p jc-tui --example undo_pipeline --release --locked --offline
target\release\examples\undo_pipeline.exe
```

La fuente no administra ficheros de medidas ni modifica configuración del SO o
resolución de temporizadores. Cada captura confirmó las 496 filas,
controles correctos y ausencia de full repaint antes de resumir cada etapa por
documento/operación. Se pueden mostrar distribuciones agrupadas, manteniendo
visible qué documentos y muestras las componen. Repeticiones con orden invertido
ayudan a identificar variación de máquina/planificador; no se presentan como
aislamiento perfecto de la carga del equipo.

## Resultado local — 2026-11-02

El código plano `d6de969` se exportó a una raíz aislada bajo work y enlazó la
misma fuente del probe, verificada por SHA-256. Actual: índice productivo por
bloques, base `191bd82` más la migración identificada en metadata. Rust 1.85.1
MSVC, release del proyecto, locked/offline. Orden: bloques/plano y plano/bloques.
Cuatro capturas de 496 filas: **1984 operaciones conservadas**, controles completos.
Clippy del ejemplo y build release pasaron. No se añadió API al producto.

P95 agrupado de 248 operaciones por fila, µs; cada valor es primer/segundo par:

| Operación y etapa | Plano | Bloques |
|---|---:|---:|
| Enter, handle_event | 302,6 / 311,4 | 26,2 / 25,8 |
| Enter, frame | 151,1 / 148,3 | 135,6 / 132,5 |
| Enter, encode | 39,6 / 44,3 | 22,3 / 22,7 |
| Enter, recorrido total | 474,2 / 483,4 | 182,2 / 178,2 |
| Undo, handle_event | 427,9 / 584,4 | 22,9 / 23,0 |
| Undo, frame | 149,3 / 147,2 | 135,1 / 130,7 |
| Undo, encode | 38,0 / 40,3 | 22,4 / 22,8 |
| Undo, recorrido total | 573,7 / 752,9 | 179,2 / 172,1 |

El resumen también conserva cada documento por separado. Los percentiles por
etapa no se suman para calcular total: total es otro reloj por operación.
La mejora aparece en ambos órdenes para esta escena calentada, sin aislar por
completo asignador/carga del equipo ni generalizar a cualquier texto.

El mismo RunAcceptance se repitió con ocho procesos por versión (uno descartado),
bloques primero y plano después. Los 14 retenidos aprobaron 8 ediciones, 8 undos,
8 cierres limpios, SHA-256 de disco y salida cero cada uno. Undo inyección-a-VT:
56 muestras por versión, p95 **16,9702 ms plano / 16,9147 ms bloques**; máximos
22,2286 / 22,6254 ms. La captura previa por bloques había dado 20,9661 ms de p95
frente al plano histórico 16,5021 ms. La diferencia no persistió en la repetición;
queda variabilidad de cola nativa, sin atribución causal al núcleo ni al SO.
Pico histórico de compromiso repetido: 52,313 / 43,820 MiB respectivamente.
No se descarta la primera captura ni se mezclan sus muestras con estas.

Reproducir resúmenes: `python scripts/summarize-undo-pipeline.py`, con destinos
ausentes; valida identidades, cobertura, no-full y checks antes de escribir.
[Resumen por etapa/documento](benchmarks/M2-undo-pipeline-summary.csv),
[resumen VT separado](benchmarks/M2-undo-native-repeat-summary.csv),
[raw bloques r1](benchmarks/M2-undo-pipeline-block-r1-samples.csv),
[raw plano r1](benchmarks/M2-undo-pipeline-flat-r1-samples.csv).
Los cuatro prefijos `M2-undo-pipeline-{flat,block}-r{1,2}` incluyen raw/checks/metadata;
los dos `M2-evidence-dense-edits-{flat,block}-repeat-native` incluyen
memory/timings/checks/metadata. Ningún dato representa pantalla física/GUI,
aceptación global de RAM ni una resta de percentiles para fabricar latencia residual.

# M2-F — Edición densa por teclado y memoria retenida

Método y harness revisados/compilados por el integrador. El recorrido nativo se
ejecutó completo en las versiones anterior y actual, con datos separados y sin
compilaciones/fixtures concurrentes. Resultados y limitaciones al final.

## Alcance y propiedad

`scripts/conpty-acceptance.cs` amplía el harness ConPTY existente, su `Display` y
su host privado. Conserva `RunAcceptance(executable, host, directory, shape,
iteration)`, los modos `before`, `after`, `dense-before`, `dense-after`, los
corpus anteriores y las columnas anteriores de tiempos/memoria. No cambia APIs
del editor, cuotas productivas, renderer, Git ni el perfil release.

Los modos CLI `dense-edits-before` / `dense-edits-after` seleccionan la misma forma
`dense-edits` y distintos prefijos de datos. Genera
ocho documentos ASCII de exactamente 4 MiB cada uno: primera línea de marcador
`M2_VISIBLE_n ` seguida de 128 `x` y LF, luego repeticiones de `abcdefg\n` hasta
el tamaño exacto, con el último bloque truncado si corresponde. Es el mismo
corpus que `dense`; la primera línea larga permite conservar el cronometraje
previo de Right. Las líneas restantes tienen ocho bytes, salvo el fragmento
final. No se normaliza EOL ni se agrega un newline terminal diferente.

Todos los generadores ahora escriben bloques de 64 KiB y restan un contador
`remaining`; no consultan `FileStream.Length` por cada línea de ocho bytes.
Como las unidades previas dividen 64 KiB, los bytes de los corpus anteriores
permanecen iguales. La preparación ocurre antes de iniciar el reloj/ConPTY.

Las rutas de fixture son nuevas por PID, forma e iteración bajo
`work/m2-native-PID-dense-edits-ITERATION`; se rechaza un directorio existente y
cada fuente se crea con `CreateNew`. La limpieza enumera solo los archivos
propios y elimina el directorio sin recursión. Los datos usan el prefijo nuevo
`docs/benchmarks/M2-evidence-dense-edits-after-native`; los CSV también se abren
con `CreateNew` y se rechaza la presencia previa de cualquiera de sus salidas.
No se guardan los documentos desde el editor ni se modifican fuentes del usuario.

## Recorrido observable

Cada proceso abre los ocho documentos UTF-8, espera el marcador visible y una
respuesta a Right, crea dos vistas con Split Right, realiza la serie previa de
36 Right y visita los ocho documentos. Conserva las fases `ready`, `split`,
`visit_documents` y `retained` del harness anterior.

El recorrido adicional usa exclusivamente teclado y los handlers centrales:

1. `before_edit`: espera 100 ms con todos los documentos y ambas vistas vivos.
2. Para cada documento, Alt+1…8 selecciona su pestaña; Ctrl+G, `1:1`, Enter
   confirma documento y posición inicial. Enter inserta un LF al inicio; se
   espera `Ln 2 Col 1`. Right y Left comprueban navegación posterior sin editar.
3. `after_edit`: espera 100 ms tras editar los ocho documentos; sus historiales
   y documentos continúan vivos. `verify_disk_after_edit` verifica por streaming
   los ocho SHA-256 contra los obtenidos antes del arranque del editor.
4. Para cada documento, Alt+1…8 selecciona la vista en `Ln 2 Col 1`; un Ctrl+Z
   debe volver a `Ln 1 Col 1`. Right y Left comprueban navegación tras undo.
5. `after_undo`: espera 100 ms con documentos e historial aún retenidos.
   `verify_disk_after_undo` repite la comprobación SHA-256 del disco.
6. One Group elimina la segunda vista; ocho Ctrl+W deben cerrar los documentos
   sin responder a ningún diálogo de cambios pendientes, hasta `Group 1: Empty`.
   `closed` espera 100 ms con el editor vivo y sin documentos.
7. File / Exit debe terminar el host con código cero; cualquier espera fallida,
   hash diferente o código distinto aborta la corrida con salida no cero.

Los hashes prueban contenido de disco intacto, incluidos EOL y newline terminal.
La vuelta del caret y cierre limpio después de un único Ctrl+Z por documento
son evidencia observable del undo en memoria; no equivalen a un hash del buffer
interno ni sustituyen las pruebas byte-preserving del núcleo.

## Salidas y lectura correcta

Se conserva un proceso de calentamiento (iteración 0, no retenida) y siete
procesos retenidos (1…7). Los CSV se escriben solo tras completar cada recorrido;
una salida parcial tras error no acredita el bloque completo.

El CSV de tiempos conserva `shape,run,operation,sample,elapsed_ms` y las filas
anteriores `startup_ready` y `injection_to_vt`. Añade ocho filas por operación y
proceso; en estas filas `sample` es el índice de documento 0…7:

| operation | Intervalo medido |
| --- | --- |
| `edit_document_start_navigation` | Alt+digit hasta confirmar Ctrl+G / 1:1 |
| `insert_initial_newline` | Enter hasta `Ln 2 Col 1` |
| `after_edit_navigation` | Right hasta `Ln 2 Col 2` |
| `undo_document_navigation` | Alt+digit hasta documento y `Ln 2 Col 1` |
| `undo_initial_newline` | Ctrl+Z hasta `Ln 1 Col 1` |
| `after_undo_navigation` | Right hasta `Ln 1 Col 2` |

Left de retorno, hashing y esperas de retención quedan fuera del tiempo de cada
operación. No se descartan calentamientos adicionales de las nuevas operaciones:
son una edición y un undo por documento, no una distribución de operaciones
repetidas sobre el mismo documento. Ctrl+G y selección están separados del tiempo
de mutación. Estos tiempos llegan hasta VT observado por el harness; no miden
pantalla física. La apertura previa ya puede calentar índices/cachés del editor y
del SO; no se afirma arranque realmente frío.

El CSV de memoria mantiene sus columnas originales. Cada fila distingue editor,
host privado, conhost propio y harness; cada uno tiene private bytes, working set
y peak commit. No se suman como memoria del editor. `after_edit`, `after_undo` y
`closed` permiten comparar memoria privada/working set actualmente retenidos.
`peak_commit` es un máximo histórico del proceso: sigue alto tras liberar y no
describe retención de la fase. Los muestreos secuenciales con pausa solicitada
de 5 ms pueden perder picos breves y no son medidas simultáneas de todos los
procesos. Hashing y listas de muestras pertenecen al harness; sus fases separadas
evitan atribuir ese trabajo al editor. Las esperas de 100 ms no fuerzan GC ni
purga del working set o caché del sistema.

El CSV adicional `-checks.csv` contiene:

```text
shape,run,edited_documents,undone_documents,cleanly_closed_documents,disk_unchanged_after_edit,disk_unchanged_after_undo,exit_code
```

Cada uno de los siete procesos retenidos debe producir `8,8,8,true,true,0`.
El contrato tipado adicional de `AcceptanceResult` ofrece esos contadores,
booleanos y `operations`; los campos previos permanecen disponibles.

## Ejecución reservada y límites

El integrador compilará el conjunto habitual de scripts y ejecutará el modo en
una única ventana serial, después de cerrar el bloque productivo relacionado:

```text
csc /nologo /codepage:65001 /out:work\m2-native.exe scripts\conpty-smoke.cs scripts\conpty-git.cs scripts\conpty-git-metrics.cs scripts\conpty-acceptance.cs
work\m2-native.exe . dense-edits-after
```

Debe conservar identidad/hash del ejecutable productivo, entorno y muestras
crudas, comprobar las siete filas de checks y resumir por fase/proceso. No hay
segunda implementación de launcher ni procesos auxiliares nuevos; el modo usa
el host/editor/conhost ya propios del harness.

Este método observa la ruta nativa integrada, no el allocator de un modelo
aislado. No infiere un objetivo global menor de 50 MiB por una cifra del modelo,
por sumar capacidades lógicas, por una fase favorable ni por memoria tras cierre.
No acredita GUI, latencia física, otras máquinas, Linux ni aceptación completa
de M2A. La entrega inicial del worker quedó sin ejecución; el cierre posterior
del integrador y sus datos se registran a continuación.

## Cierre medido y comparación equivalente

csc compiló el conjunto completo con .NET Framework del equipo. Se ejecutaron
ocho procesos por versión: un calentamiento descartado y siete retenidos.
Anterior: release de `6cf6f13` exportado/compilado offline en work; actual:
release de `d6de969`, mismo Rust 1.85.1 MSVC. Se usó el mismo RunAcceptance,
corpus y acciones. La etiqueta CLI before se añadió tras la captura actual para
separar el prefijo y elegir el ejecutable anterior; no cambió el recorrido medido.
Cada metadata identifica los binarios/hashes del harness usados en su captura.

Los **14 procesos retenidos** aprobaron 8 ediciones, 8 undos, 8 cierres limpios,
dos comprobaciones SHA-256 de disco y salida 0. Se conservaron 641 muestras de
memoria anteriores y 638 actuales. El resumen excluye contadores cero de procesos
ya terminados/no disponibles en vez de interpretarlos como consumo cero.

Memoria del editor; MiB = 1048576 bytes:

| Observación | Anterior | Actual |
|---|---:|---:|
| Antes de editar: rango de medianas por proceso | 49,375–49,922 | 49,395–49,895 |
| Tras editar: rango de medianas por proceso | 65,445–65,922 | 49,738–50,434 |
| Tras undo: rango de medianas por proceso | 65,445–65,934 | 49,711–50,426 |
| Cerrados: rango de medianas por proceso | 1,199–1,996 | 1,238–2,012 |
| Máximo histórico de compromiso reportado por proceso | 68,160 | 52,461 |

La política acotada reduce retención tras editar y el máximo histórico observado
en este corpus. **La meta global de 50 MiB sigue abierta**: incluso la retención
actual tras editar supera 50 MiB en algunas sesiones y el máximo histórico llega
a 52,461 MiB. El valor bajo tras cerrar no sustituye la carga con documentos vivos.
El working set y contadores del bootstrap/conhost/harness se conservan separados;
no se suman como si fueran memoria privada del editor.

En la captura actual: 217 movimientos previos, p95 inyección-a-VT 16,1111 ms.
Las 56 inserciones tuvieron p95 16,0820 ms y los 56 undos 16,5021 ms. Son muestras
de siete procesos × ocho documentos, con correlación y resolución del harness;
no son tecla-a-pantalla física ni certificación de un umbral de 16 ms.

Datos anteriores: [checks](benchmarks/M2-evidence-dense-edits-before-native-checks.csv),
[memoria](benchmarks/M2-evidence-dense-edits-before-native-memory.csv),
[tiempos](benchmarks/M2-evidence-dense-edits-before-native-timings.csv),
[metadata](benchmarks/M2-evidence-dense-edits-before-native-metadata.json).
Actuales: [checks](benchmarks/M2-evidence-dense-edits-after-native-checks.csv),
[memoria](benchmarks/M2-evidence-dense-edits-after-native-memory.csv),
[tiempos](benchmarks/M2-evidence-dense-edits-after-native-timings.csv),
[metadata](benchmarks/M2-evidence-dense-edits-after-native-metadata.json).
[Resumen por fase/proceso](benchmarks/M2-growth-native-phase-summary.csv) y
[resumen de tiempos](benchmarks/M2-growth-native-timing-summary.csv) se generan
con scripts/summarize-growth-evidence.py, que valida integridad y checks completos.

Esta evidencia justifica comparar un índice por bloques antes de elegir otra
estructura: el índice plano mantiene cuatro bytes por línea y el sufijo O(L).
El candidato permanece experimental fuera del editor hasta medir memoria/coste
y preservar offsets/EOL/lecturas; no se declara implementada esa arquitectura.

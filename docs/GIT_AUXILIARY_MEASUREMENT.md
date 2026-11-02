# Medición del editor y auxiliares Git

Componente opt-in preparado en `scripts/conpty-git-metrics.cs`, 2026-11-02.
Propiedad del worker: ese archivo y este método. El integrador conecta el probe
al recorrido ConPTY existente, compila con csc/.NET Framework del equipo y realiza
la ventana serial. **El worker no ejecutó csc, procesos, fixtures ni benchmarks.**
No se añade ETW, dependencia, instalación, consola, servicio o configuración.

## API y alcance

Clase global `GitAuxiliaryProbe : IDisposable`: constructor con el PID del editor,
Phase setter, Start/Stop, Rows (IReadOnlyList), Truncated y Error. Es one-shot;
Stop/Dispose son idempotentes, unen el thread y conservan los datos. Rows devuelve
una vista de una copia consistente de referencias a strings; tras parar queda
congelada y sigue disponible después de Dispose.

La captura observa exclusivamente el editor indicado y descendientes cuya cadena
padre/hijo se pudo validar. No busca procesos Git globales ni toma sus contadores.
Toolhelp enumera únicamente metadatos de PID/PPID/nombre para descubrir candidatos.
Abre candidatos con QUERY_LIMITED_INFORMATION + SYNCHRONIZE; antes de consultar RAM
confirma PPID/nombre en otro snapshot tras fijar el handle y valida su creación
contra el padre conocido. Esa confirmación evita atribuir una vida que reutilizó
el PID entre el primer snapshot y OpenProcess. Ambos contadores y tiempos se leen
mediante el mismo handle, no reabriendo un PID para cada propiedad.

El constructor debe recibir el PID mientras el editor propio todavía está vivo:
solo con un entero no puede identificar una vida anterior que terminó antes de
construir el probe. Se fijan PID + start_ticks de esa vida y se mantienen abiertos
los handles observados hasta Stop. Para un padre terminado, la creación del hijo
debe quedar dentro de su intervalo creación/salida. Un hijo anterior al padre se
descarta. Esto evita atribuir huérfanos de un PID reutilizado a una vida distinta;
véase la advertencia de [Microsoft sobre PID/PPID reutilizados](https://devblogs.microsoft.com/oldnewthing/20150403-00/?p=44313).

No abre memoria fuente ni termina/suspende procesos: solo consultas de información.
Los permisos requeridos para contadores están documentados en
[GetProcessMemoryInfo](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo).
Las carreras de salida entre snapshot y consulta pueden omitir un proceso; no
inventan una fila cero. Fallos de acceso/identidad quedan en Error y detienen la
captura, conservando las muestras completas anteriores.

## Datos y cotas

Header exacto, fuera de Rows:

```text
sample,phase,elapsed_ms,role,pid,start_ticks,parent_pid,name,private_bytes,working_set_bytes,peak_commit_bytes,cpu_total_ms
```

Cada ciclo fija sample, phase y elapsed_ms antes de recolectar; todas sus filas
comparten esos valores. Se solicita espera de 5 ms entre ciclos, sin alterar timer
resolution del sistema. El trabajo de captura y el planificador añaden tiempo:
los timestamps retenidos determinan la cadencia real. Las lecturas de procesos
son secuenciales, no una fotografía atómica de RAM simultánea.

- role es `editor` o `auxiliary`; name identifica Git y sus wrappers concretos.
- start_ticks son ticks UTC de DateTime (100 ns desde 0001-01-01), derivados del
  creation FILETIME Windows. No se usa un PID solo como clave temporal.
- private_bytes es PrivateUsage, working_set_bytes WorkingSetSize y
  peak_commit_bytes PeakPagefileUsage del proceso observado.
- cpu_total_ms es kernel + user acumulados desde el nacimiento de ese proceso,
  no CPU exclusiva de la fase, del worker ni del período muestreado. Las unidades
  proceden de [GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes).
- phase/nombre se escapan como CSV, incluidos quotes y saltos; números usan cultura
  invariante. Phase se limita a 128 caracteres y Error al primer diagnóstico de
  hasta 1024 caracteres. No se imprime fuente ni argumentos del usuario.

Cotas: 4000 muestras, 32000 filas, 8 Mi caracteres UTF-16 de CSV contando newline
y header; también 4096 identidades con handles fijados y 65536 entradas Toolhelp
temporales. Alcanzarlas marca Truncated y detiene captura. No se publica una
muestra cortada a medias por una cuota. Son límites del probe, no RAM del producto:
strings UTF-16, listas, metadatos temporales y handles cuestan memoria adicional en
el harness. Stop señala cancelación y hace join sin abandonar el thread; una
consulta del sistema puede retrasarlo y no se promete deadline duro de parada.

## Lectura correcta de resultados

Sumar private_bytes de todas las filas de una misma muestra para estimar editor
+ auxiliares concurrentes; informar editor y cada PID/start_ticks por separado.
No sumar picos de procesos distintos y llamarlo pico simultáneo. El compromiso
máximo de cada proceso es un máximo de su vida; puede anteceder a la fase/probe.
El máximo de las sumas muestreadas es solo un máximo observado, no el pico real.

No sumar cpu_total_ms de todas las filas: repetiría CPU acumulada. Agrupar por
PID/start_ticks y usar diferencias o máximos con sus límites. Un Git que nace y
muere entre muestras no deja fila; tampoco se conoce toda su CPU final. Un padre
no observado antes de salir puede impedir probar la ascendencia de otro proceso,
que se excluye conservadoramente. No se infiere coste cero de ausencia de filas.
El instrumento añade trabajo al harness y puede perturbar scheduling/cachés.

La primera corrida es descriptiva, con tamaños y fases del integrador. Registrar
corpus, muestras reales, identidades observadas, Error/Truncated y los límites de
cobertura. Mantener separados tiempos del modelo, inyección-a-VT y pantalla física.
Este componente no mide pantalla física, no certifica RAM global <50 MiB ni cierre
M2A, no sustituye la evidencia previa de bytes/guardado/undo y no amplía Git a
operaciones con escritura. La aceptación física/global permanece abierta.

## Primera ventana local del integrador

Ejecutada en Windows con el release de `6cf6f13`, Rust 1.85.1 MSVC y Git
2.54.0.windows.1. No hubo compilaciones/fixtures paralelas. Se conservan
[procesos crudos](benchmarks/M2-git-auxiliary-first-processes.csv),
[recorridos](benchmarks/M2-git-auxiliary-first-runs.csv),
[resumen por fase](benchmarks/M2-git-auxiliary-first-summary.csv) y
[metadatos/hashes](benchmarks/M2-git-auxiliary-first-metadata.json).

Tres sesiones por tamaño, nueve en total. Cada fixture tiene primera línea larga,
Unicode y resto de líneas ASCII de 64 bytes; HEAD contiene el original, index
otra primera línea y el buffer recibe un carácter sin guardar. Se miden las fases
before/prepare/head/head_ready/navigate/stale/index/index_ready/after. El `after`
conserva intencionadamente el resultado index aceptado: no es la memoria tras clear.
Las fases son intención del harness fijada al comenzar cada muestra; una captura
lenta puede atravesar una transición de UI o el nacimiento de otro proceso.

| Fuente de trabajo antes de editar | Sesiones | Máximo privado observado editor + descendientes |
|---|---:|---:|
| 4 KiB | 3 | 4,594 MiB |
| 1 MiB | 3 | 8,434 MiB |
| 2 MiB − 64 bytes | 3 | 11,156 MiB |

Son máximos de sumas del mismo ciclo, no suma de máximos históricos ni percentiles.
Incluyen Git y sus `conhost.exe` descendientes; excluyen el bootstrap y conhost
iniciales de la TUI y el harness. El rótulo inicial del metadata decía conhost sin
esa distinción: se aclaró después de inspeccionar los nombres, sin alterar filas,
filtros ni contadores. Los hashes conservados identifican la compilación de captura
anterior a esa corrección de rótulo.

446 filas de proceso / 335 ciclos completos. En cada sesión se observaron entre
3 y 9 identidades Git con contadores y entre 4 y 7 conhost; no es un censo de todos
los comandos lanzados. Cadencia real de los ciclos: mediana 31,4027 ms, p95
47,8086 ms y máximo 64,1638 ms, pese a solicitar espera de 5 ms. No se corrigió el
timer del sistema. Esta cobertura puede omitir procesos y picos breves, y no mide
un pico simultáneo atómico: cada consulta del ciclo sucede secuencialmente.

Las nueve sesiones aprobaron HEAD/index sin otra tecla, navegación, undo→Stale,
hunks conservados al ocultar/reabrir el panel, disco/index sin cambios y salida 0.
Error/Truncated quedaron vacíos/false en todas. No se repitió la suite funcional
del editor; no hubo cambios productivos para obtener este corpus. La construcción
del ejemplo de sufijos tiene su ventana independiente y no se mezcló con la captura.

Reproducción local, sin PowerShell si el host presenta el bloqueo conocido:

```console
csc /nologo /codepage:65001 /out:work\measure-git-auxiliary.exe scripts\conpty-smoke.cs scripts\conpty-git.cs scripts\conpty-git-metrics.cs scripts\measure-git-auxiliary.cs
work\measure-git-auxiliary.exe RAIZ RUTA_ABSOLUTA_GIT PREFIJO_NUEVO_DENTRO_DE_RAIZ
python scripts/summarize-git-auxiliary.py PREFIJO
```

El runner rehúsa sobrescribir datasets; genera nueve sesiones y CSV/metadata.
El resumen comprueba recorridos y cuotas antes de calcular sumas observadas.
La primera captura es descriptiva y no acepta <50 MiB globales: falta ampliar
documentos/piezas/líneas extremas, mezcla con otros servicios y cobertura temporal.
Pantalla física, IME/grafemas/DPI y otras plataformas conservan sus pendientes.

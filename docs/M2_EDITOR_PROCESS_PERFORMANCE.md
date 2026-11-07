# M2: memoria y rendimiento del proceso completo — 2026-11-06

Este bloque mide los ejecutables reales GUI/TUI, además del modelo instrumentado
histórico. Usa exclusivamente fixtures propios y procesos nuevos en segundo plano;
no activa ventanas, teclado global, portapapeles, recuperación personal ni red.
Las compilaciones y mediciones se serializan y reutilizan las dos cachés existentes.
Ryzen 9 8945HS, 16 procesadores lógicos, Windows 10.0.26300, Rust 1.88 MSVC,
perfil release existente. Carga de otras aplicaciones desconocida y sin modificar.

## Corrección de retención

Al abandonar Markdown mediante otro comando o cambiar de documento, la siguiente
escena ahora cancela su solicitud y libera los píxeles aceptados inmediatamente.
Antes descartaba la vista pero dependía de un próximo poll de tareas para liberar
la caché; ese evento podía no llegar. Cancelar vacío es idempotente y evita tomar
el mutex/incrementar generaciones en cada escena de texto. No se espera al worker
desde la UI ni se cancela trabajo de otros servicios.

La regresión usa referencias débiles a los píxeles decodificados y demuestra su
liberación en ambas rutas sin ejecutar poll. El frame retenido anterior debe
soltarse al reemplazar la escena, como ya hace el propietario nativo. El hilo y
GDI+ pueden seguir dormidos hasta cerrar la aplicación; no son píxeles retenidos.

## Protocolo GUI

`scripts/measure-m2-editor.py` crea fixtures y lanza serialmente el ejecutable
real con `--smoke-performance-only`, dentro del modo de prueba oculto existente.
El probe opera en el mismo proceso/hilo de UI: consume el mismo frame, renderer
y DIB retenido mediante el mensaje estándar WM_PRINTCLIENT/PRF_CLIENT y un DC/DIB
propio de medición. No usa BeginPaint/EndPaint ni publica hits Markdown durante
impresión. Comprueba píxeles no negros, cero errores y composiciones reales.

Fue necesario descartar dos prototipos: WM_PAINT oculto puede tener clip vacío
y omitir composición; la impresión/captura entre procesos también produjo una
superficie vacía en este entorno. Sus logs se conservan y **no acreditan render**.
Tampoco se usa el título del SO para verificar cambios: CIDE dibuja su título
cliente. El medidor final comprueba inserción y restauración de revisión/longitud
en el modelo real, sin copiar toda la fuente ni alterar los contratos de Undo.

Un proceso por corpus: 4.095 bytes; 4 MiB de líneas cortas; 4 MiB UTF-8 con
multibyte/emoji; una línea de 16 MiB; ocho documentos de 4 MiB. En el último se
visitan las ocho pestañas, reteniendo documentos y proyecciones. Se registran
120 desplazamientos sintéticos y 32 ciclos de edición/deshacer con composición,
dos segundos de reposo despachando la cola con espera bloqueante, cierre limpio
de los documentos y salida del modo de prueba existente (no una prueba nueva
de recuperación/FileExit). Las muestras se conservan individualmente. Una línea
no representa desplazamiento vertical efectivo ni cursor al final de esa línea.

Private/working set/peak commit son contadores del SO para el proceso del editor,
incluyendo heap y memoria nativa. **Incluyen también el DIB adicional del probe**,
cuya cantidad exacta se publica; no se resta de RSS ni se presenta una cifra
ajustada como medición. Hay fases antes de crearlo y después de liberarlo, entre
las nueve muestras de fase. El DIB retenido normal de CIDE sigue incluido.
Los picos del SO son acumulados y capturan picos entre muestras. Cerrar documentos
no obliga a Windows a devolver inmediatamente las reservas del heap. Un proceso
por corpus y caché del SO sin controlar: no es distribución de arranque frío ni
de rendimiento entre ejecuciones independientes.

## Resultados GUI

Superficie de 1000×650; DIB adicional del medidor de 2.600.000 bytes (2,48 MiB),
incluido sin descontar en todos los picos siguientes. La tabla usa compromiso
privado, no working set. Muestras calientes dentro de una ejecución por corpus;
sin comparación antes/después ni afirmación de mejora de throughput.

| GUI | Pico privado/compromiso MiB | p95 rueda→composición ms | p95 editar+deshacer+dos composiciones ms |
|---|---:|---:|---:|
| Pequeño | 7,53 | 2,78 | 5,63 |
| Denso 4 MiB | 12,45 | 3,04 | 5,69 |
| Unicode 4 MiB | 11,98 | 5,46 | 12,17 |
| Una línea de 16 MiB | 24,58 | 1,18 | 2,69 |
| Ocho documentos de 4 MiB visitados | 46,00 | 2,09 | 5,13 |

Los cinco procesos devolvieron 0. Se comprobaron 949 composiciones nativas,
cero errores de pintado, 600 desplazamientos y 160 ciclos de edición/deshacer.
Los doce archivos conservaron sus bytes originales. En multi, cerrar documentos
bajó la memoria privada de 46,00 a 8,52 MiB; liberar el DIB de medición la dejó
en 6,04 MiB, conservando la superficie normal de CIDE. No es una garantía contra
crecimiento tras cientos de sesiones ni un límite del heap. El working set
máximo de multi fue 57,91 MiB: no confundirlo con compromiso privado <50 MiB.

El contador de CPU no avanzó durante los cinco intervalos de ~2 s en reposo,
con espera bloqueante y despacho de eventos pendientes. Es una observación
acotada a la resolución del SO, no una promesa de CPU cero permanente.

El objetivo local del editor base <50 MiB queda satisfecho en estos corpus y
esta geometría, incluso con el DIB adicional. No convierte el umbral en una
aceptación universal. El tiempo de impresión excluye BeginPaint/compositor y
no certifica tecla→pantalla física. Solo hay un viewport activo por momento.

## Protocolo TUI

`scripts/measure-m2-tui-process.py` reutiliza el host y parser ConPTY privados
existentes, 240×40 celdas. Mide solo el proceso hijo CIDE: host, conhost y parser
quedan explícitamente fuera. Corpus denso y UTF-8 de 4 MiB, y línea de 16 MiB
tras un encabezado identificable. Registra 63 muestras después de un calentamiento,
ocho ciclos de editar/deshacer comprobados en VT, PageDown, un segundo de reposo
y salida normal sin guardar. Los tres archivos conservaron sus bytes originales.
Las teclas y ediciones ocurren en el encabezado ASCII inicial, con el corpus
abierto; PageDown sí visita su contenido. No mide edición de la línea larga ni
movimiento dentro de grafemas multibyte. Esos contratos conservan sus pruebas
dirigidas anteriores, sin repetirlas ni atribuirles estos tiempos.

El parser de prueba existente no modela ancho doble/combining: se conservó la
corrida fallida y se limitó este corpus UTF-8 a glifos multibyte de ancho sencillo.
No se cambió el renderer para satisfacer el parser. La primera geometría de
120 columnas ocultaba el status por la longitud de la ruta del fixture; por eso
se usa 240. No se cuentan esos intentos como aceptación.

| TUI | Pico de compromiso MiB | Arranque hasta VT ms | p95 tecla→status VT ms |
|---|---:|---:|---:|
| Denso 4 MiB | 6,93 | 94,26 | 16,10 |
| Unicode 4 MiB | 6,12 | 76,28 | 16,08 |
| Línea larga 16 MiB | 17,95 | 137,34 | 16,06 |

Reposo: 0–15,625 ms de CPU durante ~1 s, resolución del contador incluida. El
endpoint VT incluye transporte, temporización y parser; no equivale al coste
del modelo, al compositor ni a tecla→fotón. No se certifica el objetivo físico
de 16 ms a partir de estas cifras.

## Límites de aceptación

El objetivo de <50 MiB del plan corresponde al editor base. No equivale a limitar
todo uso simultáneo: el DIB admite 32 MiB (3840×2160 ocupa 31,64 MiB) y Markdown
puede mantener hasta 16 MiB aceptados más 16 MiB de entrega, además de fuentes,
frames y memoria nativa. Las cuotas lógicas no son un límite RSS. Este corpus
no activa simultáneamente búsquedas, Git, imágenes, recuperación ni proveedores.

La memoria/rendimiento con esos servicios simultáneos y geometrías extremas,
la latencia física y el traslado real entre monitores conservan su aceptación
pendiente. IME excluido por Juan; Narrador/accesibilidad avanzada aplazados.
Ninguna observación de este bloque cierra por sí sola M2 físico/global.

## Validación y reproducción

- Seis pruebas dirigidas GUI de medios Markdown aprobadas, una nueva de liberación.
- Formato y Clippy GUI all-targets con warnings denegados aprobados.
- Workspace release locked/offline aprobado; caché `target/build-cache` reutilizada.
- TUI: tres procesos ConPTY privados, 189 muestras y 24 ciclos editar/deshacer;
  salida normal por Archivo → Salir, código 0. Las guardas posteriores de rutas
  no modifican el protocolo; sintaxis/AST verificada sin repetir la medición.
- GUI: cinco procesos ocultos del ejecutable real, 949 composiciones y código 0;
  no se repitió la suite HWND completa ni la suite de runtime del workspace.

```powershell
python scripts/measure-m2-editor.py --exe target/m2-process-next/release/cide-gui.exe
python scripts/measure-m2-tui-process.py --exe target/m2-process-next/release/cide-tui.exe --host target/release/examples/f3_console_host.exe
```

Entrega `target/m2-process-next/release`; binarios anteriores preservados. La TUI
medida en `target/m2-editor-performance-next/release` tiene SHA256 idéntico a
la entrega final; no se repitió su corrida por cambios exclusivos de GUI.
[Datos GUI](evidence/m2-editor-gui-process-2026-11-06.json) y
[datos TUI](evidence/m2-editor-tui-process-2026-11-06.json) conservan muestras,
fases, hash del ejecutable y límites. Logs originales y primeros intentos en
`work/m2-*`; ningún fallo inicial se cuenta como evidencia aprobada. No se
accionaron ventanas antiguas ni se acreditó su salida. Sin dependencias, lock,
CI, push, portapapeles o depósitos personales nuevos.

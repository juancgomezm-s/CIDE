# Explorer compartido implementado — 2026-11-03

El [contrato previo](M2B_EXPLORER_CONTRACT.md) ya tiene implementación en TUI y
GUI Windows. `jc-app::ProjectExplorer` es el único catálogo/proveedor del árbol;
`ExplorerView` posee selección y scroll, y prepara filas de solo lectura. Se retiró
la enumeración síncrona y las aperturas alternativas del Explorer TUI. Quick Open
conserva su búsqueda por query; no alimenta el árbol. Sin dependencias nuevas.

## Uso y propiedad

File / Open Folder establece una raíz explícita. View / Explorer muestra y enfoca
la sidebar; Ctrl+Shift+E también está registrado. Flechas/Home/End/Page recorren,
Right expande o entra al primer hijo; Left/Backspace colapsa o selecciona padre.
Enter alterna directorios o abre archivos, Ctrl+Enter abre al lado. F5 refresca el
directorio seleccionado o padre; Escape cancela su carga pendiente y vuelve al
editor. En GUI, F6 alterna foco; click selecciona, disclosure alterna expansión y
dobleclick abre. Menú/palette conservan el contexto de la selección del árbol.

La apertura conserva dirty DocumentId, undo, pin, vistas y navegación de Workbench.
No hay clon de Document/FileSession/undo ni payload leído para construir el árbol.
Una entrada que cambió a directorio/reparse se rechaza antes de activar incluso un
documento ya abierto. La elección de encoding reutiliza el flujo existente. Tab,
texto y paste con foco Explorer no editan el documento oculto.

NodeId combina generación de raíz y ordinal no reutilizado. Refresh y collapse
invalidan descendientes; raíz nueva invalida el catálogo completo. `poll` acepta
solo generación/revisión vigente y estado Queued/Loading; cancelar no admite una
completion tardía aunque el proveedor terminara justo antes de recibir la señal.
Paths nativos se conservan como OsString/PathBuf; las etiquetas ASCII escapan
Unicode, controles, barras y nombres no escalares por tokens completos. Nunca se
construye un path desde un label.

## Trabajo y presentación acotados

| Recurso | Implementación inicial |
| --- | --- |
| Catálogo | 4096 nodos y 1 MiB de capacidades heap de nodos/paths/children IDs/raíz |
| Worker | Uno lazy, una petición activa; ocho pendientes/128 KiB, coalesce por directorio |
| Entrega | Una completion/512 KiB con backpressure; owner acepta máximo dos por poll |
| Enumeración | Hijos inmediatos; 2048 retenidos/512 KiB, 50 000 examinados, deadline cooperativo 2 s |
| Profundidad/expansión | 32 niveles, 32 directorios expandidos; collapse expulsa descendientes |
| Presentación | Hasta 128 filas × 256 columnas; indentación visual limitada al ancho disponible |

Se declaran aparte staging de enumeración de hasta 512 KiB y scratch Windows de
64 KiB. Son cotas lógicas de recursos propios, no medición de memoria privada.
Orden directorios primero y nombre nativo; Limited ordena solo el subconjunto
retenido, sin prometer paginación ni primer bloque lexicográfico global. `.git`
queda excluido explícitamente; no hay watcher ni indexador recursivo permanente.
Deadline/cancelación/Drop con join son cooperativos: una llamada de FS bloqueada
puede retrasarlos. El deadline comienza al admitir, también para peticiones en cola.

Solo el poll de tareas acepta entregas; prepare/draw no enumera, expande ni consume
completions. La GUI publica hit targets únicamente al pintar su propio HWND:
generación, revisión y NodeId. Resize/refresh/root invalidan targets antiguos.
La sidebar reserva área al editor; cliente compacto la oculta, conservando split,
DocumentId, carets y undo. La preparación offscreen no publica targets de ratón.

## Límite del sistema de archivos

`jc-files::list_directory_in_root` enumera Windows con
GetFileInformationByHandleEx, anclado al handle del directorio. Mantiene abiertos
raíz y ancestros ordinarios con OPEN_REPARSE_POINT y sin FILE_SHARE_DELETE; verifica
tipo y confinamiento antes de publicar. Apertura de archivos vuelve a validar la
cadena y el handle de hoja, conservando la validación por handle de FileSession.
No se siguen symlinks/junctions/reparse ni se admiten dispositivos o `..`.

Esto no congela el contenido del directorio ni constituye snapshot de FS o CAS
global ante un adversario. El fallback no Windows usa read_dir y revalidación de
metadata: conserva una ventana de namespace y no fue ejecutado en este Windows.
La evidencia local incluye junction real, nombre UTF-16 no escalar y rename real
de un ancestro rechazado mientras el handle está fijado. El symlink opcional del
smoke GUI no se creó por privilegio Windows 1314; no se cuenta como check aprobado.

## Evidencia local

Se construyó catálogo, proveedor, vista, migración TUI, sidebar GUI y ambos arneses
antes de abrir una ventana integral de validación. Rust 1.85.1 MSVC x64, offline:

- **352 pruebas aprobadas**, cero fallos, dos helpers de subproceso ignorados;
  suites completas de jc-app/jc-files/jc-tui/jc-gui, no corrida de todo el workspace.
  Trece casos Rust nuevos y retiro de un test del proveedor TUI eliminado.
- fmt, Clippy workspace/all-targets con warnings denegados y release workspace
  bins/examples aprobados. Los tres avisos iniciales de Clippy quedaron corregidos.
- **70 checks GUI nativos** en HWND propio oculto: doce nuevos de Explorer y los
  58 anteriores; seis pinturas reales, cero errores. Fixture de 13 bytes y hashes
  de metadata `.git` sintética intactos.
- **Seis recorridos TUI en ConPTY propia**, salida cero, fuentes intactos: entrega
  sin otra tecla, expansión, apertura, refresh/cancel, dirty reuse/undo y resize.
  Se corrigieron dos esperas del arnés que confundían título/pestaña con menú/fila.
- Seis ayudas/aliases y siete frames de icono por cada uno de los seis PE aprobados.

Logs crudos, fallos iniciales, reportes y hashes de fuentes/binarios:
[metadata](evidence/m2b-explorer-2026-11-03-metadata.json),
[GUI](evidence/m2b-explorer-2026-11-03-native-report.json),
[ConPTY](evidence/m2b-explorer-2026-11-03-conpty-report.json).
[Panel expandido y split](evidence/m2b-explorer-2026-11-03-native-explorer.png) es
conversión BMP GDI→PNG sin pérdida, revisada visualmente; no captura de pantalla física.

ZIP local de desarrollo: `work/packages/CIDE-windows-dev-explorer-2026-11-03.zip`,
3 836 852 bytes,69 miembros, CRC y hashes del manifiesto verificados.
[Hash y límites de notices](evidence/m2b-explorer-2026-11-03-package.json).
Conserva dos notices GNU no disponibles en el inventario offline; no es instalador
ni certificación de distribución o release.

No se repitieron perfiles cerrados ni suites aprobadas por documentación/estilo.
M2A/M2B continúan abiertos: clipboard OS, IME, DPI/accesibilidad y aceptación global
RAM/pantalla siguen pendientes para el final. Este bloque no acredita parser,
LSP, navegación ABL o loader de extensiones; tampoco instala .NET, servicios o CI.

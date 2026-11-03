# Host GUI de Problems/Output y foco — 2026-11-03

Bloque posterior a `8fbd879`, previsto por el [inventario](M2B_GUI_INVENTORY.md).
Consume el contrato F3 existente: no añade proveedor de negocio, AVM, compilador,
servicio ni dependencia. Un panel vacío es el estado normal sin productor conectado.

## Contrato y procedencia

`Workbench` posee un `TaskPanels` con los dos logs. `PanelRow` posee texto una sola
vez, ID monotónico y origen opcional: TaskStamp (DocumentId/revisión/encoding/ViewId),
ruta nativa y rango de bytes. La fila Problems tiene destino diagnóstico real;
Message/ProposedEdit/resumen de Output conservan origen con rango0..0 y **no**
habilitan navegación. Logs locales/sistema carecen de destino. Texto de salida
es dato, incluso si parece comando; ProposedEdit se describe y nunca se aplica.

Se retiraron los Vec<String>/append_log y la stringificación F3 de la TUI. Ambos
frontends llaman a `accept_task_completion` después de separar completions Git.
En GUI un único `jobs.poll(...,4)` vive en el dispatcher F3 de `poll_git`, invocado
desde TASKS_MESSAGE. Git conserva su controller y reserva de artefactos; el resto
va al almacén compartido. Preparación/dibujo no admiten ni pollean trabajos.

`JobSystem` captura también la ruta al admitir: guardar como puede conservar
revisión y encoding, pero cambia la identidad pertinente del resultado. Hasta4096
unidades nativas admitidas para esa ruta. Cancel_stale comprueba stamp y ruta;
la entrega y la activación comprueban de nuevo. No se añade path mutable al worker
ni otra copia del documento. Los límites F3 de snapshots/resultados/colas siguen
vigentes; la metadata de ruta de sus registros está acotada por trabajos/ruta.

Problems/Output: **256KiB de heap lógico por log**, incluyendo capacidad del Vec,
structs PanelRow, String y PathBuf retenidos; máximo1000/2000 filas y4096 bytes UTF8
por texto, con marcador de truncamiento. Cuota de log no equivale a RAM privada
del proceso ni incluye buffers de proveedor/frame. Evicción FIFO contabilizada;
IDs no se reutilizan tras clear/reset, evitando que un click viejo elija otra fila.
Selección/scroll son por ID y panel, sin capturar documentos/sesiones/undo.

## Estados y navegación

- Vacío: mensaje explícito, sin iniciar worker al mostrar el panel.
- En curso: contador F3 outstanding (incluye trabajos Git); no se inventa un estado
  queued/running por proveedor que la API no expone.
- Completed: filas aceptadas y resumen; sin items se informa completion vacío.
- Error/limit/invalid range/panic/timeout/cancel/stale: estado sistema breve en
  Output; payload rechazado no se publica. F3 entrega un lote atómico, no streaming
  parcial. Recorte/evicción posterior del log se muestra con contadores en título.
- Resultados retenidos obsoletos: prefijo `[stale]`; todavía se pueden leer, no
  activar. Raíz nueva cancela jobs y resetea logs en ambos frontends; completions
  antiguas solo pueden producir estado de descarte, sin revivir su contenido.

Cada fila F3 muestra archivo, DocumentId, revisión, encoding y rango. El label
escapa controles y es solo presentación. Enter/dobleclick en Problems usa el ID,
busca destino vigente y valida documento abierto, revisión, encoding, asociación
de vista, ruta y ambas fronteras textuales del rango. No permite interior de CRLF,
reapertura de disco ni recarga de buffer dirty. Después `jump_to_location` conserva
política de pin/historial/grupos y cambia solo la posición/vista, no bytes/undo.
Output no navega aunque tenga origen. TUI Enter en Problems comparte esta API.

## Entrada, foco y composición

View/Palette: Problems, Output, Toggle Bottom Panel. Ctrl+J muestra/oculta; al
mostrar enfoca el panel si cabe. Alt+1/2 dentro del panel cambia Problems/Output.
F6/Shift+F6 recorren Editor → Explorer visible → panel visible, en ambos sentidos;
si una región está oculta o no cabe, se omite. Escape devuelve foco al editor y
conserva panel/resultados. Mostrar/ocultar no cancela ni reinicia proveedores.
No se ofrece botón Run/Refresh/Cancel genérico: no hay productor de negocio que
asignar a esas acciones; cancelación/versiones/reinicio son APIs existentes F3.

Arriba/Abajo/PgUp/PgDn/Home/End seleccionan filas. Click usa IDs publicados solo
por WM_PAINT y comprueba kind, geometría actual y existencia; reset/evicción/resize
no convierten hits viejos en índices nuevos. Campos/overlays tienen prioridad,
conservan su flujo y ocultan input a la fuente. Paleta conserva intención de origen;
editar fuente exige Editor/editor_input, no panel/Explorer/campo. Navegación o apertura
explícita exitosa devuelve foco al editor; cancelar un campo conserva su contexto.

El panel ocupa la zona inferior del editor, a la derecha de Explorer; el editor
revela viewport antes de preparar. En ventana demasiado baja se oculta físicamente
sin perder selección/visibilidad pedida y el foco vuelve a Editor. Preparación
limita a128 filas y512 columnas; GDI consume frame de solo lectura. Escapes y
prefijos no son fuente ni coordenadas editables. No fuente editable nativa adicional.

## Validación

Se construyeron y cablearon almacén, APIs, entrega, TUI, foco, teclado/menú/paleta,
layout, GDI/hits y sección nativa antes de ejecutar checks. Worker existente solo
en task_panels.rs; un integrador en API/State/rutas/manifests/evidencia/Git.

La ventana dirigida aprobó **23 casos Rust distintos**: cuatro nuevos del almacén,
ocho jobs y nueve Git diff afectados por path/dispatcher, un flujo TUI F3 ampliado
con navegación/foco/stale y el parser CLI GUI afectado. No corrida global ni suites
de Explorer/Search/rectángulos cerradas repetidas. fmt/Clippy workspace-all-targets
con warnings denegados y release workspace bins/examples aprobados.

La sección nativa `bottom` usa JobSystem, Wakeup y mensajes reales sobre su propio
HWND, con productores **sintéticos exclusivamente de prueba**. No acredita Check
Syntax/AVM ni servicio de negocio. Prueba vacío/menu/paleta/tres focos, protección
de fuente, entrega/nav dirty/split/pin, stale revisión/encoding/ruta, root/cancel,
fallo/cuota/reinicio y propuestas inertes, recorte/IDs/hits/resize/readonly/limpieza.
Save As se ejecuta solo sobre un memdoc y destino nuevo propio para probar cambio
de ruta; verifica sus bytes y elimina ese archivo exacto. No Save del fuente original
ni clipboard OS. La primera sección aprobó13 checks; la revisión posterior añadió
origen visible y una aserción/canvas Problems, conservando la corrida inicial.
El resultado final y artefactos se registran al cerrar este documento.

Resultado final: **13 checks HWND aprobados**, seis nuevos y siete de arranque/
pintura, dos WM_PAINT y cero errores. Fuente original13B y metadata Git sintética
intactos. La segunda corrida afectada añadió origen visible/aserción Problems;
antes se ajustó el fixture vacío para comparar workers previos, no suponer cero
si se integra tras otros recorridos. No se repitieron Rust ya aprobados por esas
modificaciones GUI. Clippy/release finales y formato aprobados; no hubo fallos
funcionales en esta ventana. Las corridas inicial y final quedan separadas.

[Hashes/logs](evidence/m2b-bottom-2026-11-03-metadata.json),
[reporte final](evidence/m2b-bottom-2026-11-03-native-report.json),
[reporte inicial](evidence/m2b-bottom-2026-11-03-native-initial-report.json),
[Problems con procedencia](evidence/m2b-bottom-2026-11-03-native-bottom-problems.png),
[Output acotado](evidence/m2b-bottom-2026-11-03-native-bottom.png),
[fuente restaurada](evidence/m2b-bottom-2026-11-03-native-frame.png).
PNG1000×650,1384×741 y984×641 revisados visualmente, convertidos sin pérdida del
BMP GDI real. No son capturas de pantalla física ni métricas de latencia/RAM.

M2A/M2B no se declaran cerrados: proveedores internos, terminal, regex sin permiso
de dependencia, entrada avanzada de selectores/menús y selección general siguen
pendientes. Tampoco se certifican clipboard OS/rectangular externo, Unicode general,
pantalla/mouse físicos, IME/DPI/accesibilidad/Linux o RAM global. Symlink opcional
omitido y dos notices GNU pendientes del ZIP se conservan. Sin CI/push/publicación,
instalaciones, cambios de seguridad, proveedores ni dependencias nuevos.

Paquete local de desarrollo `work/packages/CIDE-windows-dev-bottom-panels-2026-11-03.zip`:
4.043.661 bytes, SHA-256 `bef9b3eb87da4ec42c81e1a3234a48233179c4ae0bb03ee1794ba0f8cdb4043b`.
CRC y hashes del manifiesto de69 entradas verificados; [resultado](evidence/m2b-bottom-2026-11-03-package.json).
Dos notices GNU pendientes, sin certificación de distribución/instalador.

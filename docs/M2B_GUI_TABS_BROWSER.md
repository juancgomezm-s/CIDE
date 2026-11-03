# Marco y pestañas GUI; explorador de apertura TUI

Base `bf2de28`, 2026-11-03. El usuario autorizó continuar GUI y construir en paralelo
un explorador TUI a partir de su diálogo «Abrir carpeta». La barra GUI existente
concatenaba hasta12 títulos y no tenía objetivos individuales de selección/cierre.
El criterio M2B de pestañas y grupos admite esta integración sin proveedor nuevo.

Corrección explícita del usuario durante el bloque: rechaza el aspecto de TUI
dibujada como GUI y pide una interfaz gráfica diferente, cercana a su captura de
Sublime. Se amplía este bloque a la base gráfica: Segoe UI para chrome, Consolas
para código, fondo elegido `#2E3440`, sidebar oscura con iconos geométricos y pestañas
con esquinas ligeramente redondeadas. El catálogo de documentos abiertos se
filtra en selector independiente de proyecto. La TUI no determina geometría GUI.
El núcleo y sus comandos/controladores siguen compartidos.

El repintado se compone en un DIB persistente acotado a32MiB y se publica de una
vez; los tramos ASCII contiguos de igual estilo se agrupan en hasta256 caracteres,
sin fragmentar grafemas Unicode. Se comparó contra la referencia por celda sobre
el mismo frame propio; ningún resultado sintético certifica fluidez física.
El requisito posterior de archivos de GB requiere otra revisión de almacenamiento,
carga/índices/guardado y se audita en paralelo antes de afirmar soporte.

La barra de título se integra al cliente porque DWM no aceptó el atributo de color
en este entorno. Título y menús usan el mismo fondo; los controles de ventana tienen
objetivos propios y el sistema conserva arrastre, doble click y resize mediante
hit-testing Win32. El encabezado duplicado de documento se retiró; la pestaña lleva
su identidad y el estado inferior muestra la referencia anclada.

## Contrato implementado

GUI: barras discretas por grupo visible sobre el catálogo compartido. Cada barra
muestra selección del grupo, cambios sin guardar y cierre explícito. Flechas de
desbordamiento permiten llegar a todo el catálogo; los cambios por teclado revelan
la pestaña elegida. Click izquierdo selecciona o solicita el cierre por el flujo
existente; no arrastre/reordenamiento, middle-click ni autoscroll de fuente.
El grupo compacto muestra únicamente la barra del grupo visible. Se conservan
caret/selección/viewport persistentes de F1, pin y un único documento/undo.

Los hits proceden del último frame preparado, con geometría e identidades
versionadas; resize/cambio de modelo/modal impide usar objetivos obsoletos.
El cierre sucio conserva la confirmación nativa y revalidación existentes. Pintar
solo consume labels/hits propios y no consulta documentos. Labels se recortan por
grafemas y ancho; máximo32 pestañas por barra, dos barras, sin decodificar fuente.

TUI: selector modal con directorio, ruta editable, lista acotada y ayuda visible.
Reutiliza ProjectExplorer independiente y su worker/Wakeup cancelable; render no
enumera el filesystem. Una ruta se obtiene de NodeId vigente, nunca de su label.
La selección de carpeta cambia proyecto solo al aceptar, archivos se abren por la
ruta existente y Esc conserva el contexto anterior. No escritura, watcher ni
dependencias nuevas. Los límites y casos definitivos se registrarán al integrar.

## Validación local

GUI:48 tests de biblioteca cubiertos por suites/cierres dirigidos, incluidos dos
de recuperación; no una corrida global única después de todos los cambios.
El selector acotado se probó con5000 títulos abiertos. Navegador TUI: cuatro pruebas
unitarias y cinco checks ConPTY. Ratón: seis unitarias y siete checks ConPTY.
Check all-targets, fmt, Clippy y build release offline aprobados.

Shell HWND propio:15 checks,4 pinturas y0 errores. Interacción:13 checks,1 pintura
y0 errores. Se conservaron fuentes fixture y metadatos Git. Informes finales:
`evidence/gui-shell-browser-2026-11-03-shell.json` y `*-interaction.json`.

El probe final comparó el mismo frame:2089 celdas frente a1476 tramos, raster
idéntico. En21 muestras intercaladas después de3 pares de calentamiento: celdas
p50 6.649ms/p95 7.846ms; tramos p50 5.595ms/p95 6.500ms. DIB5,304,000B, una reserva
para dos composiciones; otra al resize. Título y toolbar verificaron #2E3440 en
píxeles. Los recursos GDI de frío a después del probe fueron19→24; este dato no
demuestra ausencia de fugas ni aceptación global de RAM. Los tiempos son de GDI
offscreen con GdiFlush y no miden entrada a pantalla física.

Capturas propias actuales: [dos grupos](evidence/gui-shell-browser-2026-11-03-native-shell-split.png)
y [ventana compacta](evidence/gui-shell-browser-2026-11-03-native-shell-compact.png).
Aceptación física/global, IME/DPI/a11y y proveedores M3–M5 siguen abiertos.

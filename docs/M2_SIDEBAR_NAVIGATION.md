# Navegación y desplazamiento de paneles laterales

Bloque posterior a `223b050` (Git embebido), 5 de noviembre de 2026.

## Problema y cambio compartido

Archivos no recibía rueda en GUI. Además, `ExplorerView::prepare` volvía a
revelar la selección en cada preparación, por lo que no bastaba desplazar la
interfaz: el siguiente dibujo deshacía la intención del usuario. La TUI usaba
movimiento de selección para la rueda del árbol.

El modelo compartido distingue ahora desplazamiento manual y navegación de
selección. `scroll_by` y `scroll_to` ajustan únicamente el origen, limitado
por las filas visibles y el catálogo. La presentación incluye `first`, ordinal
global de la primera fila. Las operaciones de teclado/selección vuelven a
revelar el nodo elegido. Preparar metadatos o pintar repetidamente no deshace
el desplazamiento. No se copia ni se vuelve a enumerar el directorio en dibujo.

Las filas siguen limitadas a 128; no hay segundo árbol o documento editable.
Una sustitución de raíz reinicia la vista y rechaza identidades anteriores.
Al reducir el catálogo o ampliar el área, se limita el origen al nuevo máximo.

## GUI y TUI

Archivos tiene rueda y barra vertical en ambas interfaces. Desplazar no cambia
el nodo seleccionado, foco ni cursor del documento. La GUI acumula fracciones
de rueda, respeta la preferencia de líneas/página y escala el rail por DPI.
La TUI conserva las coordenadas de celdas y la captura de ratón existente.

Los hits incluyen generación, revisión y origen visible. Un clic preparado
antes de desplazar no puede abrir el archivo que ahora ocupa esa fila. Los
arrastres validan también contexto y geometría, y se cancelan ante raíz,
catálogo, modal o tamaño obsoletos. La GUI integra la captura con los controles
nativos para no interferir con el redimensionado de ventana.

En Símbolos TUI, RePág/AvPág sin modificadores siguen recorriendo resultados;
Ctrl+RePág/AvPág conserva la navegación global entre documentos.

## Validación

El cierre selectivo pasó 254 pruebas: 10 del catálogo compartido, 78+6 GUI y
155+5 TUI. Siete regresiones nuevas cubren desplazamiento independiente,
revelado por teclado, límites/resize/raíz, rueda fraccional, captura/arrastre,
geometría DPI y atajos globales en Símbolos. Clippy de todos los destinos y
release del workspace offline aprobaron. Binarios nuevos en
`target/m2-sidebar-next/release`; los anteriores se conservan.

138 checks nativos GUI aprobaron con PATH vacío, incluidos tres nuevos con
mensajes de rueda y ratón al HWND propio. Se comprobó captura/liberación real,
foco/selección/documento conservados y rechazo de clics anteriores al scroll.
Informe `work/native-gui-smoke/20261105T230359Z-158d0e06c998`.

ConPTY pasó siete checks con 50 archivos propios y salida normal 0. La primera
ejecución esperaba el nombre completo en ancho compacto; el texto visible
estaba correctamente recortado. Se ajustó el oracle a un prefijo distintivo,
sin cambio Rust. El cierre mantuvo los 50 archivos byte a byte:
`work/sidebar-conpty/20261105T230518Z-b9eac20e/report.json`.
Logs `work/m2-sidebar-{app-tests,frontends-tests,clippy,release,native}.log`.

La regresión integral posterior pasó 683 pruebas del workspace, cero fallos;
tres helpers ignorados en ejecución directa se invocaron desde sus padres.
Formato final aprobó. Log `work/m2-sidebar-workspace-tests.log`.
El cierre se registra en `VALIDACION.md`. Las pruebas usan
catálogos virtuales y archivos propios, sin depósitos de recuperación personales.
No acreditan pantalla/ratón físicos, IME instalado ni segundo monitor.

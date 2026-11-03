# Navegación GUI compartida — 2026-11-03

Bloque posterior al Explorer `3ae903e`. La GUI admite los comandos existentes
SearchGoToPosition, NavigateBack/Forward y SearchHistoryPrevious/Next de jc-app,
sin otro modelo de documentos, historial de saltos, matcher o undo. También expone
los flags case/whole-word existentes para usar y recuperar consultas con opciones.

## Uso

- Ctrl+G o Navigation / Go to Line / Column abre `line[:column]`. Línea/columna son
  **uno-based de escalares de fuente**; columna omitida equivale a 1. BOM, CRLF,
  bytes legacy e invisibles conservan la conversión de jc-text. No son columnas
  visuales ni posiciones UTF-16 de LSP. No hay preview al teclear.
- Campo de hasta64 bytes, flechas/Home/End/Backspace/Delete, Ctrl+U borra, Ctrl+V
  pega con validación del destino. Enter acepta; Escape cancela. Cero, overflow,
  gramática inválida o posición fuera de fuente no mueven la vista. La gramática
  se comparte con TUI en `jc-app::parse_text_position`; Workbench valida la fuente.
- Alt+Left/Right, menú Navigation y paleta ejecutan atrás/adelante por el historial
  **del grupo activo**. Carets y selección se conservan según PaneManager. Un salto
  Go to desde un grupo fijado se dirige al otro grupo, como las demás navegaciones
  automáticas existentes; atrás/adelante recorre el historial del grupo activo
  sin redirigirse. Pin y split siguen gobernados por Workbench. Repetir un destino no añade un salto
  diferente; un extremo sin destino se deshabilita por el registro central.
- Alt+Up/Down en Find/Replace, menú Navigation y paleta recuperan las20 consultas
  aceptadas que ya almacena Workbench, con sus flags. Los extremos se saturan;
  no se inventa un wrap ni historial de borradores no aceptados. Sin panel se abre
  Find. En Replace se conserva reemplazo, cursor y campo activo; el cursor de la
  consulta recuperada queda al final. Alt+C/W y menú Search cambian case/whole-word.

## Propiedad y entrega

El modal de posición captura TaskStamp (DocumentId/revisión/encoding/ViewId) y
PaneId. Aceptar o editar revalida el destino; un cambio de documento/vista/revisión/
encoding/cierre exige reabrir. Escape no mueve ni restaura una posición supuesta.
El pegado suelta préstamos antes de consultar el clipboard y revalida generación
del campo más el destino al volver. Los smokes usan exclusivamente transporte
interno, sin consultar ni sustituir el clipboard OS del propietario.

Abrir Go to/Palette o ejecutar atrás/adelante abandona la navegación F3 diferida
anterior mediante `cancel_search_navigation`, conservando el scan cooperativo y
sus resultados. No puede llegar después y mover el caret detrás de un modal nuevo.
La búsqueda mantiene la validación existente de revisión/encoding/ViewId/caret al
entregar. Recuperar historia reinicia ese mismo scan; no selecciona ni reemplaza
automáticamente y no crea un segundo almacenamiento de opciones/consultas.

La paleta mueve temporalmente el único borrador Find/Replace, sin clonarlo, para
restaurarlo al cancelar o ejecutar historia/flags. Un comando diferente termina
ese borrador según el flujo modal existente. Menús verifican disponibilidad antes
de cerrar overlays. Atajos globales de navegación están admitidos también desde
Find/Replace y Explorer. Ninguno de estos comandos edita ni guarda fuentes.

Preparar y dibujar consume campos acotados; no ejecuta navegación ni acepta
completions. La GUI conserva el backend Win32/GDI y su presentación de filas;
Sublime Text sigue como referencia visual, ecode/eepp como referencia de lógica.
No hay dependencia, parser, extensión, AVM ni motor regex nuevos.

## Validación del bloque

Todos los flujos y recorridos se construyeron antes de ejecutar checks. Worker
existente reutilizado en los dos archivos exclusivos de integración GUI/TUI;
integrador único para API, State, rutas, presentación, scripts, checks y Git.
Rust1.85.1 MSVC x64 local/offline:

- **78 casos Rust distintos aprobados:**44 S0/F1/panes/workbench,30 GUI y cuatro
  regresiones TUI nuevas. No es una corrida global del workspace. Los tres TUI
  iniciales aprobados no se repitieron; solo se repitió el cuarto tras corregir
  su expectativa del pin, documentada arriba. No se cambió PaneManager.
- fmt, Clippy workspace/all-targets con -D warnings y release workspace
  bins/examples aprobados. Sin dependencias nuevas.
- **14 checks nativos aprobados**, siete nuevos de navegación y siete de arranque/
  pintura, con un WM_PAINT y cero errores. El arnés `--section navigation`
  (`--smoke-navigation-only` interno) ejecutó exclusivamente este bloque en HWND
  oculto propio. No se repitieron suites Explorer ni el recorrido nativo global.
- Inválidos/out-of-range, cancelación, límite64B atómico, repetición, menú/paleta/
  atajos, BOM/Unicode/CRLF/invisibles, pin y ambos historiales, revisión/encoding/
  ViewId/pane/cierre obsoletos; recuperación multilínea con opciones y reemplazo
  preservado, F3 diferido cancelado sin detener el scan y undo original intacto.
- Fuente propia de13B y metadatos `.git` sintéticos intactos. El guard de metadata
  no equivale a otra validación de los proveedores Git. Seis ayudas/aliases y
  siete frames por cada PE aprobados. Sin clipboard OS, CI, push o instalaciones.

[Logs/fuentes/binarios](evidence/m2b-navigation-2026-11-03-metadata.json),
[reporte nativo14](evidence/m2b-navigation-2026-11-03-native-report.json),
[campo de posición](evidence/m2b-navigation-2026-11-03-native-navigation.png) y
[fuente restaurada](evidence/m2b-navigation-2026-11-03-native-frame.png). PNG1000×650
convertidos sin pérdida de BMP GDI y revisados visualmente, sin redibujar la UI.
El fallo inicial del oráculo TUI y logs crudos también se retienen.

ZIP local de desarrollo `work/packages/CIDE-windows-dev-navigation-2026-11-03.zip`:
3 864 967 bytes,69 miembros, CRC y hashes del manifiesto aprobados.
[SHA-256 y notices pendientes](evidence/m2b-navigation-2026-11-03-package.json).
Dos notices GNU siguen sin estar disponibles en el inventario offline; conserva
los límites de licencia/distribución del empaquetador. No es un instalador ni release.

M2A/M2B siguen abiertos. Este recorrido no acredita pantalla física, clipboard OS,
IME/DPI/accesibilidad, Linux ni RAM global. No se repitieron perfiles cerrados ni
se pidieron pruebas manuales intermedias al propietario.

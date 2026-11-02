# M2B — Escena y contrato para comparar frontends

Preparación del 2026-11-02. [Abrir la escena local](../prototypes/m2b/index.html).
Es un **estudio visual con datos fijos**, sin editor gráfico funcional, backend
elegido, parser ni ejecución ABL. No acredita cierre de M2B o de los objetivos
globales de M2-F. Sublime Text conserva la dirección visual; las referencias
ecode/eepp fijadas en la [auditoría](AUDITORIA_ECODE_ARQUITECTURA.md) sirven para
flujos y arquitectura. No se copió código ni recursos de esos productos.

## Entrega y medios observados

Archivos reservados a este frente: este documento y `prototypes/m2b/`. No modifica
crates, manifests/lockfiles, exportaciones, comandos, PLAN o VALIDACION.

El workspace declara ocho crates y no contiene `jc-gui`. `Cargo.lock` contiene
crossterm 0.29 y sus dependencias, pero no un toolkit gráfico. La TUI es el único
frontend conectado al modelo en esta revisión. No se instaló ni descargó un
backend. Las búsquedas `where msedge.exe` y `where chrome.exe` no encontraron
ejecutables en PATH; eso no demuestra que los navegadores estén ausentes del
equipo. Después de terminar todas las mediciones de aceptación, el integrador
encontró Edge en `C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe`
y trató de generar una captura headless con perfil aislado bajo `work/`. El
proceso terminó con `0xC0000005` sin imagen; no se modificó seguridad/configuración
ni se instaló otro navegador. No se arrancó un navegador durante las mediciones
de aceptación.

La escena usa HTML/CSS/JavaScript sin dependencias, red, fuentes remotas, servidor
ni build. Se abre directamente como archivo local en un navegador disponible.
Los botones exteriores cambian estados ilustrativos: espacio de trabajo/palette,
grupo enfocado y ventana compacta. Esc cierra la palette de muestra. Los menús,
pestañas, resultados y comandos dibujados no ejecutan acciones. No hay `input`,
`textarea`, `contenteditable`, almacenamiento persistente ni historial de edición.

| Archivo | Función |
|---|---|
| `index.html` | Escena, advertencia de alcance y fixture JSON embebida |
| `scene.css` | Tema, métricas y geometría responsive; sin reglas de negocio |
| `scene.js` | Composición de filas y conmutación de estados de muestra |
| `check-scene.py` | Comprobación estática de identidades, rangos, encoding y recursos |

## Caso común de comparación

La escena presenta `src/order-entry.p` y `includes/session.i` anclado en el segundo
grupo, Explorer, búsqueda inferior y palette centrada. Marco compacto, editor
dominante, pestañas sobrias por grupo, foco visible y barra de estado mantienen
la dirección de Sublime sin adoptar sus shortcuts ni su sistema de plugins.

La fixture declara dos DocumentId distintos, dos ViewId, revisiones y encoding
explícito Windows-1252/CRLF sin BOM y con newline final. Es una elección del caso,
**no** la página real del usuario. El contenido ABL es ilustrativo: no compilado
ni ejecutado en OpenEdge. Los tres resaltados de `cCliente` son coordenadas
escalares explícitas del fixture; JavaScript no añade un motor de búsqueda ni
analiza fuentes de usuario. Las filas no son una copia canónica editable.

El anclaje es independiente de scroll vinculado. El modo compacto muestra el
grupo ilustrado como activo; no reduce el tamaño de letra para fingir que caben
dos editores. La escena usa fuente Consolas con fallbacks locales, 13 CSS px y
24 CSS px de altura de línea. Estas métricas describen la maqueta; no certifican
ancho de grafemas, DPI nativo o dimensiones físicas de texto.

No se simulan botones funcionales de Git, LSP, compilación, debug o regex. Tampoco
se presentan minimap, símbolos o navegación automática de includes como avances
implementados. La palette solo ilustra comandos existentes: Focus Group 1/2 y
One Group, con Ctrl+1/2 tomados del keymap actual. Ctrl+D conserva Duplicate y
Ctrl+Shift+D Delete Line en la futura escena interactiva.

## Frontera compartida: disponible y pendiente

`Workbench`, `PaneManager`, revisiones/transacciones y comandos compartidos ya
existen. La `Presentation` actual de `jc-tui` es privada y contiene geometría de
celdas y una referencia a `TuiApp`: **no es una API GUI pública lista para reutilizar**.
El dispatcher actual también conserva coordinación de overlays/frontend dentro
de TUI. M2B requiere extraer o adaptar esas fronteras una vez, sin implementar
otro ciclo de edición en un widget.

| Área | Contrato a conservar | Trabajo previo a un frontend real |
|---|---|---|
| Documento/vista | Una sesión/undo por DocumentId y caret/selección/viewport por ViewId | Resolver tabs/grupos contra esas identidades; no usar el texto de un control como fuente de verdad |
| Lectura | Rangos por revisión/encoding y proyección compartida F2 | Preparar filas visibles/margen antes de componer una escena prestada; no decodificar todo el documento al dibujar |
| Edición | Operaciones de Workbench o `apply_text_edits` por revisión esperada | Traducir intención tipada; no permitir que un widget publique bytes o lleve un undo paralelo |
| Comandos/foco | `CommandId`, registro, condiciones, keymap, overlays y ciclo Esc/F6 | Compartir coordinación de aplicación sin importar menús/celdas de TUI al nuevo renderer |
| Dibujo | Composición de solo lectura después de actualizar viewport | Cachear geometría por vista y configuración, invalidar rangos; reposo por eventos |
| Tareas | Snapshots/colas/cancelación acotados y entrega por versión/generación | Despertar el frontend; descartar resultados obsoletos antes de presentar |
| Entrada gráfica | Bytes, escalares, grafemas y columnas visuales distintos | Hit testing/selección/IME deben convertir explícitamente a posiciones válidas de la revisión vigente |
| Guardado | `FileSession`, huellas, reemplazo seguro y recuperación | Reutilizar el flujo de aplicación; el backend no escribe el archivo fuente |

Una futura escena prestada debe identificar documento/revisión/encoding/vista y
exponer filas/rangos visibles, selección, caret y metadatos. La geometría en DIP,
glyph shaping y recursos gráficos pertenecen al frontend. Una selección visual
no se convierte por cast a byte/columna escalar. La composición IME puede poseer
texto temporal acotado de preedición, pero solo una confirmación válida publica
una transacción en el documento. No duplicar el historial por cada grupo.

El fixture JSON de esta maqueta **no** propone un formato de persistencia ni una
copia completa del documento para cada frame. Solo fija los datos de la escena
que cada candidato deberá obtener del mismo Workbench real.

## Matriz neutral de candidatos

No hay comparación empírica de backends gráficos todavía. Las filas siguientes
son rutas de evaluación, no un ranking ni paquetes admitidos. Sus costes se
verificarán con prototipos equivalentes antes de introducir dependencias.

| Ruta | Disponibilidad observada | Pregunta que debe resolver | Evidencia que falta |
|---|---|---|---|
| TUI/crossterm | Conectado y compilable en el workspace; evidencia del integrador | Base común de comandos, fuentes, cuotas y cargas | No representa GUI, mouse/IME gráfico ni pantalla física |
| Ventana/render propio sobre APIs Windows | Sin adaptador gráfico en el repo; MSVC/Windows ya se usan para la TUI | ¿Puede entregar texto/shaping/TSF/UIA/DPI sin otro buffer, con coste mantenible? | Prototipo real, superficies accesibles, IME, selección, recursos y medidas |
| Toolkit de UI Rust | Ninguno declarado/fijado en Cargo.lock | ¿Permite layout visible prestado y control del documento/undo, sin obligar a otro editor? | Paquete concreto, licencia, coste de dependencias/build/distribución y pruebas equivalentes |
| Host webview con frontend gráfico | Esta maqueta es HTML independiente, no un host integrado | ¿Puede separar aplicación/Rust de presentación y acotar transporte, runtime y preedición? | Host/runtime realmente disponible, procesos auxiliares, IPC por versión y todas las medidas |
| Control editor existente | Ninguno integrado | ¿Puede consumir el documento byte-preserving sin ownership/normalización/undo propios? | Demostración del contrato; si exige otro buffer editable no supera la puerta actual |

No usar memoria/timing de este HTML o de un navegador ya abierto para justificar
la elección de un host webview. Tampoco inferir ligereza de un binario pequeño,
popularidad del toolkit o GPU habilitada. Cada candidato deberá registrar proceso
principal, auxiliares y total simultáneo, con los mismos datos y servicios apagados.

## Protocolo previo a elegir backend

1. Conectar dos vistas reales al mismo Workbench y reproducir la escena sin parser,
   AVM, LSP ni indexador obligatorio. Comparar documento compartido y dos documentos;
   mantener el mismo corpus, revisión y perfil de release.
2. Fijar fuente/tamaño/altura de línea, viewport y área cliente equivalente, por
   ejemplo 1280×800 DIP y ventana compacta 900×650 DIP. Registrar escala Windows
   100/150/200 %, monitor y rasterización. Zoom CSS no sustituye DPI/TSF nativo.
3. Capturar workspace, palette y referencia enfocada; revisar clipping, contraste,
   barra de estado, foco por teclado y fallback compacto. No usar captura como
   prueba de editor funcional o de rapidez.
4. Recorrer abrir/encoding, selección, editar, undo/redo, split/pin/cierre, búsquedas
   y reemplazo por revisión. Menú, teclado y palette deben resolver la misma acción.
   Tab/Shift+Tab en diálogos conservan navegación y no editan el documento.
5. Probar mouse/drag, portapapeles, composición IME con confirmar/cancelar, letras
   suplementarias, combining, tabs y ancho doble. UIA/lector de pantalla requiere
   comprobar roles, foco, selección y texto sin normalización de la fuente.
6. Medir arranque hasta una edición observada; separar cold/warm, preparación,
   composición y entrega. Para hot paths, al menos 5 calentamientos +31 muestras
   y p50/p95; registrar bytes/asignaciones visitados y no solo promedios.
7. Medir memoria privada/working set/picos y retención al cerrar, con varios
   documentos/vistas, líneas extremas y muchas piezas. Contar recursos gráficos,
   runtime/host y trabajadores por separado y como total simultáneo.
8. Separar latencia del modelo, entrega a API/VT y actualización física. Una marca
   de render o screenshot automatizado no acredita fotón/pantalla. Si no hay medios
   admitidos para medir pantalla física, conservar expresamente ese pendiente.

No ejecutar compilaciones/fixtures pesados de varios agentes durante mediciones.
Una ventana de ensayo se acuerda con el integrador; el cierre integra resultados
en PLAN/VALIDACION. CI remoto continúa reservado a M5.

## Validación real de esta entrega

`python -X utf8 prototypes/m2b/check-scene.py` comprueba JSON embebido, dos
documentos/vistas, revisión de búsqueda, los tres rangos escalares, codificación
estricta del contenido de ejemplo, CRLF/newline final, IDs únicos, recursos locales
y ausencia de controles editables de fuente. Es validación estática del fixture.
`node --check prototypes/m2b/scene.js` también terminó correctamente; comprueba
sintaxis JavaScript, sin ejecutar ni renderizar la escena.

No se compiló un backend ni se ejecutó un benchmark o una matriz de navegador/DPI.
La tentativa de captura headless falló sin generar imagen. La revisión visual
renderizada, interacción del navegador y accesibilidad de la maqueta permanecen
pendientes; no se presenta la comprobación estática como revisión visual. IME/UIA reales, RAM/latencia/arranque GUI y aceptación
global de M2B no se pueden concluir de estos archivos.

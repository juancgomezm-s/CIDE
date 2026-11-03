# Cierre funcional de los pendientes TUI — 2026-11-03

> Actualización 2026-11-03: [GUI/TUI en español](M2B_GUI_WORKFLOWS_ES.md)
> incorpora comparación, fuente opcional y plantillas en GUI; color/menús Alt,
> ayuda lateral y Markdown en TUI, Markdown también en GUI. F10 abre menús,
> F6 cambia panel y Ctrl+Alt+M alterna Markdown. Las matrices, cifras o pendientes
> anteriores de este documento se conservan como evidencia histórica.


Instrucción de Juan: construir bloques amplios, validar al cierre y seguir sin
otro «continúa». Los bloques del alcance admitido están implementados y validados
localmente. La aceptación física/global de M2 permanece abierta.

| Bloque | Estado y contrato |
|---|---|
| Regex Search Plus | Documento/proyecto, cuotas/cancelación, reemplazo literal. [Uso](M2_REGEX_SEARCH.md). |
| Comparar dos abiertos sin Git | Dos snapshots versionados, visor lateral/unificado y navegación. [Uso](M2_OPEN_DOCUMENT_DIFF.md). |
| Símbolos/includes/RUN | Registro interno y proveedor opcional léxico, destinos confinados/ambiguos explícitos, sin AVM. [Uso](M2_SOURCE_SERVICES.md). |
| Completado textual | Palabras locales anteriores al caret, aceptación/una transacción; semántica M4 pendiente. [Uso](M2_SOURCE_SERVICES.md). |
| Snippets | Motor universal, siete plantillas opcionales ABL, parámetros/preview/undo único. [Uso](M2_SNIPPETS.md). |
| Foco/teclado/overlays | Entrada superior, límites y navegación de controles auditados. [Contrato](TUI_INTERACTION.md). |
| Clipboard Windows lineal | Adaptador estricto compartido, owner TUI lazy con hilo Win32 responsivo; pruebas propias/inyección, interoperabilidad real abierta. [Uso](M2_TUI_CLIPBOARD.md). |
| Grafemas y rectángulos | Geometría narrow/TAB4, clusters completos, edición parcial de glifos anchos, clipping/VT incremental y cuotas. [Uso/evidencia](M2_UNICODE_LAYOUT.md). |
| Aceptación física/global | Abierta: terminal/fuente/IME/accesibilidad, clipboard entre apps, RAM/arranque/latencia de pantalla. |

Último bloque:18 casos Rust nuevos,530 nombres distintos observados aprobados
por suites y cierres dirigidos; dos helpers Git ejecutados por sus padres,
ningún caso listado sin cubrir. No corrida runtime global única. Fmt/Clippy/release
offline, cuatro flujos ConPTY y17 checks HWND propios aprobados. [Evidencia](M2_UNICODE_LAYOUT.md).

Ejecutable revisable `target/release/cide-tui.exe`; paquete de desarrollo y hashes
en [metadatos](evidence/m2-unicode-layout-2026-11-03-metadata.json). No instalador,
publicación ni certificación de distribución (dos avisos GNU pendientes).

Siguiente puerta concreta: aceptación física con terminal/fuente/clipboard y
fuentes legacy de codificación confirmada por el usuario. No se puede acreditar
esa experiencia mediante HWND ocultos/VT. Se entrega la construcción local sin
solicitar otro «continúa» ni inventar un resultado físico.

Check/Compile/Run/Schema requiere proveedor/perfil/herramientas OpenEdge reales
M3; sin ellos permanece deshabilitado. Parser/semántica/LSP/depurador M3–M4 no
son promesas derivadas de servicios léxicos. Git escritura/conflictos y CI M5;
runtime externo post-M5. Help/Build sin proveedor no tienen handler ficticio.
Shell embebido, multicaret persistente y nuevas ampliaciones de ratón no están
admitidos por esta orden; la TUI usa la terminal anfitriona. GUI de servicios de
fuente/snippets/comparación de abiertos sigue independiente del cierre TUI.

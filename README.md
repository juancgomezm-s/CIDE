# CIDE

[Memoria y rendimiento del proceso completo](docs/M2_EDITOR_PROCESS_PERFORMANCE.md).
[Medición con servicios simultáneos](docs/M2_EDITOR_SERVICES_PERFORMANCE.md).
[Pintado de ventanas grandes](docs/M2_GUI_EXTREME_GEOMETRY.md).
[Git avanzado · índice y exportación de parches](docs/GIT_ADVANCED_FOUNDATIONS.md).
[Crear un commit local](docs/G4_CREATE_COMMIT.md).
[Grafo de commits](docs/G1_GRAPH_VIEW.md).

[Markdown renderizado: imágenes, sangrías y enlaces](docs/M2_MARKDOWN_MEDIA_LINKS.md).

[Búsqueda de commits](docs/G3_COMMIT_SEARCH.md) y
[caché de compilación/limpieza local](docs/LOCAL_BUILD_CACHE_CLEANUP.md).

[Consulta histórica Git compartida](docs/G1_HISTORICAL_READS.md): revisión, archivos
y comparación de solo lectura en GUI/TUI, con núcleo consumible sin el editor.

[Selector con tema y espacio Git nativo](docs/M2_GIT_WORKSPACE_NATIVE.md):
historial local, archivos y comparación en áreas independientes.

[Marco GUI y pestañas](docs/M2B_GUI_TABS_BROWSER.md),
[ratón y explorador TUI](docs/M2_TUI_MOUSE.md) y
[recuperación de sesiones en Rust](docs/SESSION_RECOVERY.md) integrados con evidencia
local. Fondo elegido `#2E3440`; título y menús GUI comparten el tema. Ambas interfaces
conservan cambios sin guardar, historial y vistas al salir y reabrir.
Recuperación periódica y limpieza segura de copias: [contrato y evidencia](docs/PERIODIC_RECOVERY_RETENTION.md).
GUI/TUI actuales en `target/git-rename-patch-refs-next/release/`, con
[motor Git embebido en Rust](docs/GIT_EMBEDDED_ENGINE.md), sin `git.exe`.
[Rueda y barras del Explorador](docs/M2_SIDEBAR_NAVIGATION.md) conservan selección y foco.
[Deshacer/rehacer restaura la selección](docs/M2_SELECTION_HISTORY.md), también al recuperar la sesión.
Ejecutables anteriores conservados. [Estado Git GUI](docs/M2_GIT_GUI_STATUS_UX.md).
[Pestañas, panel Código derecho y menús depurados](docs/M2_SHELL_PANELS_MENUS.md).
Archivos de varios GB: fuente paginada aislada probada, integración al editor pendiente.
Aceptación física/global y proveedores AVM/semánticos siguen abiertos.

CIDE es el nombre actual del proyecto, antes JC-IDE. Comandos canónicos: `cide`
(CLI), `cide-tui` y `cide-gui`. Los ejecutables `jc-ide*` siguen como aliases de la
misma entrada. La carpeta/repositorio/remoto y crates `jc-*` no se renombraron.
[Compatibilidad y marca](docs/CIDE_REBRAND.md). Icono JG+CIDE original verde para
TUI y variante naranja para GUI: [recursos y procedencia](assets/branding/README.md).


Editor de código ligero, con núcleo Rust, soporte de varios lenguajes e integración
prioritaria con Progress OpenEdge/ABL. El diseño conserva los bytes y la codificación
original de los fuentes, incluidos los que no son UTF-8.

**Estado: M2 en desarrollo — F1/F2/F3 implementados con evidencia local: vistas independientes, transacciones, rangos/índices, cuotas, presentación de solo lectura, terminal incremental y tareas acotadas/versionadas. Metas globales de rendimiento abiertas. Search Plus de proyecto ya busca por lotes y ofrece reemplazo literal con preview y conflictos; S0 literal ya usa tramos cooperativos y case folding completo Unicode 17. Regex, comparación de abiertos, servicios léxicos opcionales, snippets, clipboard explícito y layout por grafemas están integrados en TUI. Pendientes funcionales del alcance TUI implementados con evidencia local; aceptación física/global de M2A abierta. GUI con dirección visual de Sublime Text, comparación de abiertos, servicios de fuente opcionales, plantillas y Markdown integrados con evidencia local.**
Este repositorio reúne la investigación, decisiones, plan y una implementación inicial
real. No es aún un reemplazo de PIEW, Sublime Text, Notepad++ o VS Code.

## Qué contiene

| Componente | Implementado en esta base |
|---|---|
| `jc-core` | Piece table de bytes, edición, revisiones, undo/redo acotado y compactación explícita |
| `jc-text` | UTF-8/Windows-1252/ISO-8859-1/IBM850, decodificación no destructiva, line endings, coordenadas explícitas y helpers de grafemas |
| `jc-platform` | Transporte Windows CF_UNICODETEXT estricto compartido y propietario lazy TUI |
| `jc-files` | Sesión de archivo, detección de cambios externos, guardado atómico y creación no-clobber |
| Selecciones | Port parcial de `Region` de Lime Text, con procedencia y licencia conservadas |
| `jc-languages` | 18 perfiles y detección por extensión/nombre de archivo |
| `jc-progress` | Construcción de argumentos sin shell, solicitudes ASCII y parser acotado de diagnósticos/esquema |
| `jc-app` | Comandos, keymap, foco, paneles, tareas y Search Plus de proyecto con preview/reemplazo versionado; compartido por TUI/GUI |
| `jc-tui` | Frontend terminal en M2A: edición, tabs, overlays, Explorer, resultados, menús y command palette |
| `jc-gui` | Frontend Windows Win32/GDI: edición compartida, split, selectores, Search Plus, Git de solo lectura y Explorer lazy |
| Auxiliares ABL | Código experimental para comprobar/compilar y exportar esquema; requiere pruebas con Progress real |
| `jc-cli` | Inspección de archivos, copia exacta, edición de bytes a copia nueva, demo de undo/redo, preparación de solicitudes y validación de respuestas Progress |
| Documentación | Resumen, investigación, arquitectura, decisiones, hitos, lenguajes y evidencia de validación |

El núcleo usa la biblioteca estándar de Rust; el frontend terminal usa crossterm. La piece table conserva un vector de fragmentos con índice por bloques para localizar rangos en O(log P); editar aún reconstruye metadatos. El historial tiene límites configurables de pasos (1024) y carga lógica por bytes (8 MiB); la recolección de fuentes conserva los undos retenidos. Véanse [F2 y sus mediciones](docs/F2_RANGES_INDEX_MEMORY.md) y la [auditoría histórica](docs/AUDITORIA_ECODE_ARQUITECTURA.md).

## Empezar

Instala [Rust](https://rustup.rs/), clona el repositorio y abre su carpeta. Se fija el
toolchain 1.88.0; en Windows, usa el entorno de compilación MSVC requerido por Rust.

```console
cargo test --workspace --locked --offline
cargo run -p jc-cli --offline -- demo
cargo run -p jc-cli --offline -- languages
cargo run -p jc-cli --offline -- inspect ruta/al/programa.p
```

Copiar un documento sin decodificar ni alterar finales de línea:

```console
cargo run -p jc-cli --offline -- roundtrip entrada.p copia-nueva.p
```

Insertar un byte `F1` en la posición 0 de una **copia nueva**, conservando el original:

```console
cargo run -p jc-cli --offline -- edit entrada.p 0 0 F1 copia-editada.p
```

Los offsets del núcleo son bytes, no caracteres ni columnas visuales. `jc-text` realiza las conversiones de coordenadas de forma explícita y `jc-files` implementa el ciclo de guardado seguro/atómico para el futuro frontend. La CLI de demostración conserva sus salidas `create_new` deliberadamente.

## Checkpoint de navegación en M2A

La TUI dibuja dos grupos Primary/Secondary con cursor, selección y viewport independientes. Comparten `TabId`, `FileSession` y undo; los offsets se ajustan al editar y deshacer. Pin/unpin, scroll vertical vinculado, apertura lateral y back/forward se ejecutan desde el registro central, menús y palette. En terminales estrechas se muestra el grupo activo conservando el otro. El diff del archivo activo frente a HEAD/index ya está conectado en modo de solo lectura; símbolos e includes/RUN léxicos ya están integrados en TUI mediante el proveedor opcional local, sin AVM. M2A conserva abiertas sus puertas globales.

F1 retiró los campos espejo de cursor/selección/scroll del documento. Los grupos poseen vistas persistentes con ViewId y comparten el documento/undo. Replace-all publica una sola transacción; cerrar una vista preserva otras referencias. F2 ya sustituyó la decodificación completa de navegación/render por proyección compartida y rangos visibles; F3 separa preparación/render, emite diferencias y entrega trabajos internos sin otra tecla. Véanse [avance F3](docs/F3_PRESENTATION_TASKS.md), [avance F1](docs/F1_DOCUMENT_VIEWS_TRANSACTIONS.md), [arquitectura](docs/ARQUITECTURA.md) y [plan](docs/PLAN.md).

En este equipo se preparó Rust 1.88.0 dentro de `work/toolchain`, sin cambiar el PATH del usuario. El wrapper PowerShell también admite un Cargo ya instalado:

```powershell
.\scripts\cargo-local.ps1 run -p jc-tui --locked --offline
.\scripts\cargo-local.ps1 test -p jc-app -p jc-core -p jc-tui --locked --offline
.\scripts\cargo-local.ps1 fmt --all '--' --check
```

`work/` está ignorado por Git; en un clon nuevo se necesita instalar Rust. El wrapper no descarga herramientas. Las pruebas de guardado seguro requieren permitir hard links y `ReplaceFileW`; un sandbox que los bloquee puede devolver acceso denegado.

La [evidencia ampliada M2](docs/M2_ACCEPTANCE_EVIDENCE.md) mide corpus Unicode/legacy, muchas piezas/documentos, memoria por proceso y arranque; corrige el pico del layout frío. El [índice compacto y la recolección sin copia innecesaria](docs/M2_DENSE_LINE_METADATA.md) reducen el pico nativo denso de 71,9 a 51,7 MiB; la aceptación global sigue abierta.

La [medición de sufijos](docs/M2_SUFFIX_COST_REVIEW.md) detectó y corrigió crecimiento duplicado de metadatos al insertar newline: ahora se reserva holgura acotada. En el modelo denso de 8 MiB la capacidad de proyección posterior baja de ~8 a ~4,03 MiB; no equivale a RAM privada ni mejora general de latencia. La [primera captura editor + auxiliares Git](docs/GIT_AUXILIARY_MEASUREMENT.md) aporta nueve sesiones descriptivas con límites de cobertura. La [distribución fría comparada](docs/M2_COLD_GROWTH_DISTRIBUTION.md) no muestra un cambio consistente de signo entre versiones; la [edición nativa de ocho documentos](docs/M2_DENSE_EDIT_EVIDENCE.md) reduce el máximo histórico de compromiso de 68,16 a 52,46 MiB, todavía sobre la meta. La [migración por bloques](docs/M2_BLOCK_LINE_INDEX.md) ya sustituyó el índice plano: pico denso editado de 43,73 MiB y 43,82 MiB en la repetición. El [pipeline de undo](docs/M2_UNDO_PIPELINE_PROBE.md) mejora en ambos órdenes; el VT conserva variabilidad y no mide pantalla física. Se acepta ese corpus local, manteniendo abiertas las metas globales.

La especificación de doble panel, Outline, navegación de includes/RUN, autocompletado y Git está en [M2_NAVIGATION_PANES_GIT.md](docs/M2_NAVIGATION_PANES_GIT.md). Para retomar desde una PC Windows usa el [prompt de continuación](docs/CONTINUAR_EN_EQUIPO.md).

## Search Plus de proyecto

El [Explorer compartido TUI/GUI](docs/M2B_SHARED_EXPLORER.md) ya enumera hijos bajo
demanda mediante un catálogo acotado/cancelable. File / Open Folder y View /
Explorer; flechas recorren, Enter abre, Ctrl+Enter abre al lado, F5 refresca y Escape
vuelve al editor. En GUI también admite click/dobleclick y F6 para alternar foco.
Conserva documentos con cambios, pin, split y undo; no mantiene un indexador permanente.

La [navegación GUI](docs/M2B_NATIVE_NAVIGATION.md) ya ofrece Ctrl+G para línea/columna
de escalares de fuente, Alt+Left/Right para historial del grupo y Alt+Up/Down en
Find/Replace para consultas aceptadas con opciones. Menú Navigation y paleta usan
los mismos comandos. Recuperar consultas conserva el reemplazo; navegar no edita
ni guarda. Los destinos cambiados mientras el campo está abierto exigen reabrirlo.

La [edición de líneas y comentarios GUI](docs/M2B_NATIVE_LINE_EDIT.md) ya conecta
Ctrl+D, Ctrl+Shift+D, Alt+Up/Down y los comentarios del perfil a menú y paleta.
Comparte bytes y undo entre vistas; Explorer y campos modales no editan fuente
oculta. ABL usa `/* ... */` mediante metadata, sin iniciar OpenEdge.

[Selección y reapertura GUI](docs/M2B_SELECTION_REOPEN.md): Ctrl+F3/Ctrl+Shift+F3
buscan la selección literal; Ctrl+Shift+T reabre desde disco actual o vacío si era
nuevo, sin recuperar cambios descartados. Search ofrece cancelación explícita;
Navigation y paleta también exponen pestañas1…8/última, además de Alt+1…9.

El [rectángulo GUI](docs/M2B_RECTANGLE_GUI.md) ya admite Alt+Shift+flechas,
Alt+arrastrar y menú/paleta, con grid TAB4, espacio virtual y clipboard interno.
Una edición termina la selección con un undo compartido. Conserva el repertorio
por grafemas del [layout Unicode compartido](docs/M2_UNICODE_LAYOUT.md),
incluidos CJK/emoji y cortes parciales definidos. Clipboard rectangular OS sigue
fuera del alcance; clipboard lineal Windows explícito está integrado.

Edit también expone Indent/Outdent; View ofrece Link/Unlink Scroll (Ctrl+Alt+L).
El enlace aplica el delta vertical entre grupos conservando carets y pin; la barra
de estado indica `Scroll linked`. La [matriz de los 104 comandos GUI](docs/M2B_GUI_INVENTORY.md)
separa rutas reales, evidencia y pendientes. El [host Problems/Output](docs/M2B_BOTTOM_PANELS.md)
ya consume resultados F3 tipados: Ctrl+J, View/paleta, Alt+1/2 dentro del panel y
F6/Shift+F6 entre Editor/Explorer/panel. Enter navega Problems vigentes, Output es
texto inerte. Sin proveedor conectado permanece vacío; no inicia Progress/AVM.

[Menús y ratón GUI](docs/M2B_NATIVE_INTERACTION.md): menús contextuales desde el
modelo compartido, selección lineal por drag/Shift, rueda fraccionaria sobre cada
grupo y scroll enlazado sin mover carets. Validación local:16 casos Rust y13 checks
HWND; aceptación física/global todavía pendiente.

[Entrada de selectores GUI](docs/M2B_SELECTOR_INPUT.md): Quick Open, paleta y encoding
ya permiten caret/selección con Shift, Home/End, Delete y Ctrl+A/C/X/V, también por
Edit. Up/Down/Tab recorren resultados; Ctrl+Home/End elige primero/último. Campos
de256 bytes UTF8, pegado completo o rechazo sin cambios parciales; clipboard OS
real sigue sin certificar, la automatización usa únicamente texto sintético.

Abre una carpeta y usa Ctrl+Shift+F para consulta literal sensible a mayúsculas, inclusión/exclusión por globs y resultados progresivos. Enter navega; Esc en el panel inferior cancela. Tras una consulta completa sin avisos, Search / Replace in Project permite revisar sustituciones y confirmar con F9/Ctrl+Enter. Abiertos: edición con undo sin guardar; cerrados: guardado por archivo y conflictos explícitos. Véanse [uso, cuotas y límites](docs/S1_PROJECT_SEARCH.md) y [guardado Windows](docs/SAVE_WINDOWS_CONTENTION.md). En documento activo, Ctrl+F/Ctrl+H ya ofrecen búsqueda acotada, case folding completo Unicode 17, selección/historial y Ctrl+G escalar: [S0](docs/S0_DOCUMENT_SEARCH.md). Regex acotado está integrado en documento/proyecto: [uso, semántica y evidencia](docs/M2_REGEX_SEARCH.md). No se certifican normalización Unicode ni objetivos globales de rendimiento.

En Windows, abre la raíz del repositorio y usa View / Git: Active File Diff; la primera vez solicita la ruta absoluta de Git para esa sesión. HEAD/index, refresh, cancelación y configuración están en View y palette. La comparación incluye cambios sin guardar (hasta 2 MiB por fuente), muestra identidad/OID y permite navegar hunks sin editar snapshots. Cambiar documento, vista, revisión o encoding exige refresh. [Entrega y límites](docs/GIT_DIFF_DELIVERY.md), [presentación TUI](docs/GIT_DIFF_PRESENTATION.md) y [proveedor Git](docs/GIT_READ_ONLY_PROVIDER.md). El estado de repositorio de solo lectura se describe abajo; no hay watcher; operaciones Git con escritura siguen en M5. La [escena de estudio GUI](docs/M2B_SCENE_STUDY.md) sigue como estudio histórico; el [frontend nativo actual](docs/M2B_NATIVE_WINDOWS.md) es un ejecutable Win32/GDI independiente de ese HTML.

## Política de CI

El CI remoto se reserva para **M5 (hardening)**. Durante M2–M4 GitHub Actions no se dispara automáticamente por push/PR y no debe utilizarse como bucle de depuración de código todavía inestable. El workflow permanece manual y dormido hasta M5; la validación intermedia se hace local cuando el entorno lo permite.

## Varios lenguajes

Perfiles iniciales: **ABL, C#, Rust, JavaScript/JSX, TypeScript/TSX, Python, HTML, CSS,
SQL, XML, JSON, JSON con comentarios, Markdown, TOML, YAML, PowerShell, Shell y texto**.

Actualmente se reconocen los perfiles y se pueden editar sus archivos como bytes.
El resaltado léxico de **ABL, Rust, JSON y JSONC** está integrado en TUI y GUI; los demás perfiles conservan texto plano. Véase [contrato y evidencia local](docs/M2_SYNTAX_HIGHLIGHTING.md). Parsers, completado semántico, LSP y depuración siguen pendientes.
El núcleo es independiente del lenguaje y la codificación se elegirá por documento;
véase [docs/LENGUAJES.md](docs/LENGUAJES.md).

## Progress: integración pequeña, entorno real

El modelo toma como referencia la integración de PIEW: utilizar el compilador y la AVM
de Progress, consultar el esquema mediante ABL y mantener un catálogo local para navegación.
Cada proyecto seleccionará su ejecutable, `.pf`, `.ini`, `PROPATH` y páginas de códigos.

En M0, `progress-plan` **prepara una solicitud y muestra ejecutable/argumentos; no inicia
Progress, no conecta a una BD y no ejecuta programas de negocio**. Los auxiliares ABL y
la prueba manual están documentados en [progress/README.md](progress/README.md) y
[docs/PROGRESS_BRIDGE.md](docs/PROGRESS_BRIDGE.md). La ejecución de programas que requieren
el contexto de un ERP se implementará con perfiles de arranque explícitos.

La compatibilidad con OpenEdge 10.0B y 11.7 es un objetivo pendiente de validación sobre
esas instalaciones. El transporte inicial admite rutas/PROPATH ASCII; esa restricción
no cambia los bytes ni la codificación del contenido de los fuentes. No se asume que los
fuentes del usuario sean Windows-1252, ISO-8859-1 o IBM850.

## Leer primero

1. [Resumen del proyecto](docs/RESUMEN.md)
2. [Plan por hitos y criterios de aceptación](docs/PLAN.md)
3. [Arquitectura](docs/ARQUITECTURA.md)
4. [Decisiones y pendientes](docs/DECISIONES.md)
   - [Arquitectura de extensiones multilenguaje](docs/EXTENSION_ARCHITECTURE.md)
5. [Investigación de Lime Text, PIEW, ecode y Progress](docs/INVESTIGACION.md)
6. [Contrato de interacción TUI](docs/TUI_INTERACTION.md)
7. [M1: archivos, encoding y posiciones](docs/M1_FILES_TEXT.md)
8. [Validación observada y límites](docs/VALIDACION.md)
9. [M2A: checkpoint TUI](docs/M2_TUI_CHECKPOINT.md)
10. [Comandos ABL: alcance realista](docs/ABL_EDITOR_COMMAND_SCOPE.md)
11. [Search Plus: búsqueda prioritaria](docs/M2_SEARCH_PLUS.md)
12. [M2: doble panel, navegación y Git](docs/M2_NAVIGATION_PANES_GIT.md)
13. [Prompt para continuar desde el equipo](docs/CONTINUAR_EN_EQUIPO.md)
14. [Post-M5: compatibilidad Sublime Text](docs/POST_M5_SUBLIME_COMPAT.md)
15. [Auditoría de arquitectura, ecode y baseline local](docs/AUDITORIA_ECODE_ARQUITECTURA.md)
16. [F2: rangos, índices, memoria y mediciones](docs/F2_RANGES_INDEX_MEMORY.md)

## Procedencia y licencia

La parte portada de Lime se limita a las operaciones de `Region` indicadas en el código
y sus casos de compatibilidad. El buffer, historial, CLI y adaptador son trabajo nuevo.
No se incorpora la aplicación PIEW ni su motor gráfico.

El archivo LICENSE declara MIT para el código nuevo; se conservan los avisos propios de terceros.
Las condiciones BSD de la parte derivada de Lime se conservan: [LICENSING.md](LICENSING.md).

Consulta [CONTRIBUTING.md](CONTRIBUTING.md) para construir, probar y ampliar el proyecto.


El [estado Git por repositorio](docs/GIT_STATUS_READ_ONLY.md) y su
[entrega versionada](docs/GIT_STATUS_DELIVERY.md) ya están integrados/validados:
261 pruebas completas, fmt/Clippy/release locales. No identidad Document ficticia,
sin escrituras, rutas byte opacas y configuración/filtros explícitos. Presentación
TUI de status integrada: Open Folder con la raíz del repositorio y View /
Git: Repository Status (también en palette). Solicita ruta absoluta de Git para
la sesión si falta; admite refresh/cancel y selección Up/Down/Home/End sin abrir
archivos. No requiere documento ni encoding. Muestra disco/índice con rutas byte
escapadas, configuración aislada y sin renames/submódulos; no incorpora buffers
sucios. [Uso, contrato y límites](docs/GIT_STATUS_PRESENTATION.md). Cierre local:
290 pruebas completas, fmt/Clippy/release. Diff activo HEAD/index sigue operativo.


Search Plus aprovecha varios núcleos por archivo, con dos trabajadores por defecto
y límite de arranque `--search-workers 1..4`. [Resultados CPU](docs/CPU_PERFORMANCE.md):
en 16 MiB, espera mediana 1,24–1,49× más rápida y memoria privada muestreada menor,
con CPU total 30–48% mayor. Se conservan cuotas, cancelación, orden y bytes; Qwen
queda como carga desconocida, sin tocar sus procesos. `opt-level=s` se mantiene
tras compararlo. [Uso y límites](docs/CPU_RUNTIME_OPTIONS.md).

Selección rectangular disponible en Editor para el repertorio explícito de una
celda, TAB4 y columnas virtuales: Alt+Shift+flechas o Edit/Palette `Rectangle`.
Copy/Cut/Paste usan filas en el clipboard interno y una transacción por edición;
Escape cancela. La edición efectiva termina la selección. Unicode general,
ancho doble, grafemas y clipboard del sistema siguen pendientes; caracteres no
admitidos se rechazan. [Uso y límites](docs/M2_RECTANGULAR_SELECTION.md).

Edit/Palette ya ofrecen Join Lines y Split Line: Ctrl+Shift+J une líneas en
Editor; Enter divide con el EOL preferido mediante el comando compartido. Ctrl+J
conserva el panel inferior. Join elimina separadores internos/indentación ASCII,
conserva el salto final y produce un undo. [Semántica, cuotas y validación](docs/M2_LINE_COMMANDS.md).

Ya hay un **GUI Windows ejecutable** sobre el mismo Workbench: abrir/crear,
editar, seleccionar, undo/redo, guardar/Save As, pestañas y dos grupos. Backend
Win32/GDI sin framework descargado; clipboard interno y layout escalar con límites
explícitos. Binario local: `target/release/cide-gui.exe`; admite `--encoding`.
[Uso, canvas nativo y checklist manual posterior](docs/M2B_NATIVE_WINDOWS.md).

Quick Open TUI ya responde a Ctrl+P, File y Palette sobre una carpeta de proyecto;
consulta de rutas Unicode acotada/cancelable, Enter/Ctrl+Enter y revalidación antes
de abrir, conservando sesiones sucias. [Uso y cuotas](docs/M2_QUICK_OPEN.md).
El cierre local reúne339 pruebas distintas y16 checks nativos; no certifica aún
todo M2A/M2B, la pantalla física ni RAM global. Continúa adaptación Quick Open/Palette al GUI.


La GUI ya ofrece Ctrl+P Quick Open, Ctrl+Shift+P Command Palette y Open Folder;
también admite --project-root DIR. Reutiliza el finder/registro y recibe resultados
sin otra tecla, con consulta modal separada del documento. [Uso y evidencia](docs/M2B_NATIVE_SELECTORS.md):
350 pruebas completas locales y23 checks del ejecutable nativo. Continúa clipboard
del sistema y elección gráfica explícita de encoding; manual/IME/DPI siguen abiertos.


Mostrar invisibles y buscar/reemplazar bloques multilínea ya funcionan en TUI y
GUI: View/Palette/Ctrl+Alt+I, Ctrl+F/Ctrl+H, campos editables con pegado exacto,
F2 LF/F4 CRLF y F9 Replace All en un undo. Las marcas de vista no se copian ni
guardan. La GUI también ofrece elección explícita de encoding y clipboard lineal
Windows (interoperabilidad OS aún sin certificar; smoke usa memoria).
[Uso, mapas, cuotas y evidencia](docs/M2_INVISIBLES_MULTILINE.md):376 pruebas
distintas por suites, Clippy/release y29 checks nativos; sin CI ni dependencias nuevas.


Save All ya funciona en la GUI mediante Ctrl+Alt+S, File y Palette, solicitando
destino para nuevos y reportando cancelación/conflictos sin perder pendientes.
[Contrato/evidencia](docs/M2B_SAVE_ALL.md):32 checks nativos y6 pruebas nuevas;
382 casos distintos acumulados, sin declarar una corrida global382.

La GUI también ofrece reemplazo de proyecto con preview y confirmación explícita
(Ctrl+Shift+H después de una búsqueda completa): abiertos con un undo sin guardar,
cerrados por archivo con conflictos y cancelación de los pendientes. Git/Palette
ya muestran estado de repositorio y diff HEAD/índice frente al buffer sucio;
motor Rust embebido, actualización/cancelación y ninguna escritura Git.
La opción antigua `--git-executable ABS` solo se consume por compatibilidad.
[Uso y evidencia del bloque anterior](docs/M2B_PROJECT_REPLACE_GIT.md):
53 pruebas Rust dirigidas y58 controles nativos; pruebas físicas de usuario al final.

La TUI ya compara los **dos documentos abiertos sin Git**, incluidos cambios sin
guardar: View/Palette Compare Open Groups, hunks, F5, Escape y navegación a cada
grupo. Visor lateral/unificado acotado y de solo lectura; [uso y evidencia](docs/M2_OPEN_DOCUMENT_DIFF.md).
Ese visor todavía no está integrado en GUI. [Seguimiento hasta cerrar TUI](docs/M2_TUI_FINISH.md).

La TUI ofrece **Symbols**, **References in Document** y **Complete Local Word**
por menú/paleta. `progress.abl` aporta símbolos y referencias ABL inferidos;
`core.words` completa palabras anteriores al caret en cualquier lenguaje. Includes/
RUN estáticos abren candidatos confinados, con elección si son ambiguos; no ejecutan
código. El completado se acepta explícitamente y usa un undo. [Uso, cuotas y evidencia](docs/M2_SOURCE_SERVICES.md).

La TUI ofrece Insert Snippet y Surround With en Edit/Palette: parámetros, vista previa y aceptación explícita con un undo. Siete plantillas ABL opcionales; [uso y límites](docs/M2_SNIPPETS.md). No compiladas en AVM.

En Windows la TUI ya usa clipboard Unicode del sistema para Copy/Cut/Paste lineales explícitos; CtrlV también pega en campos modales. Rectángulos conservan clipboard interno. [Cuotas, errores y evidencia local](docs/M2_TUI_CLIPBOARD.md); interoperabilidad física aún abierta.

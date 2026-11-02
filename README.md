# JC-IDE

Editor de código ligero, con núcleo Rust, soporte de varios lenguajes e integración
prioritaria con Progress OpenEdge/ABL. El diseño conserva los bytes y la codificación
original de los fuentes, incluidos los que no son UTF-8.

**Estado: M2 en desarrollo — F1/F2/F3 implementados con evidencia local: vistas independientes, transacciones, rangos/índices, cuotas, presentación de solo lectura, terminal incremental y tareas acotadas/versionadas. Metas globales de rendimiento abiertas. Search Plus de proyecto ya busca por lotes y ofrece reemplazo literal con preview y conflictos; S0 literal ya usa tramos cooperativos y case folding completo Unicode 17. Regex y aceptación completa de M2A siguen abiertos. GUI con dirección visual de Sublime Text y símbolos pendientes.**
Este repositorio reúne la investigación, decisiones, plan y una implementación inicial
real. No es aún un reemplazo de PIEW, Sublime Text, Notepad++ o VS Code.

## Qué contiene

| Componente | Implementado en esta base |
|---|---|
| `jc-core` | Piece table de bytes, edición, revisiones, undo/redo acotado y compactación explícita |
| `jc-text` | UTF-8/Windows-1252/ISO-8859-1/IBM850, decodificación no destructiva, line endings y byte↔línea/columna |
| `jc-files` | Sesión de archivo, detección de cambios externos, guardado atómico y creación no-clobber |
| Selecciones | Port parcial de `Region` de Lime Text, con procedencia y licencia conservadas |
| `jc-languages` | 18 perfiles y detección por extensión/nombre de archivo |
| `jc-progress` | Construcción de argumentos sin shell, solicitudes ASCII y parser acotado de diagnósticos/esquema |
| `jc-app` | Comandos, keymap, foco, paneles, tareas y Search Plus de proyecto con preview/reemplazo versionado; compartido por TUI/GUI |
| `jc-tui` | Frontend terminal en M2A: edición, tabs, overlays, Explorer, resultados, menús y command palette |
| Auxiliares ABL | Código experimental para comprobar/compilar y exportar esquema; requiere pruebas con Progress real |
| `jc-cli` | Inspección de archivos, copia exacta, edición de bytes a copia nueva, demo de undo/redo, preparación de solicitudes y validación de respuestas Progress |
| Documentación | Resumen, investigación, arquitectura, decisiones, hitos, lenguajes y evidencia de validación |

El núcleo usa la biblioteca estándar de Rust; el frontend terminal usa crossterm. La piece table conserva un vector de fragmentos con índice por bloques para localizar rangos en O(log P); editar aún reconstruye metadatos. El historial tiene límites configurables de pasos (1024) y carga lógica por bytes (8 MiB); la recolección de fuentes conserva los undos retenidos. Véanse [F2 y sus mediciones](docs/F2_RANGES_INDEX_MEMORY.md) y la [auditoría histórica](docs/AUDITORIA_ECODE_ARQUITECTURA.md).

## Empezar

Instala [Rust](https://rustup.rs/), clona el repositorio y abre su carpeta. Se fija el
toolchain 1.85.1; en Windows, usa el entorno de compilación MSVC requerido por Rust.

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

La TUI dibuja dos grupos Primary/Secondary con cursor, selección y viewport independientes. Comparten `TabId`, `FileSession` y undo; los offsets se ajustan al editar y deshacer. Pin/unpin, scroll vertical vinculado, apertura lateral y back/forward se ejecutan desde el registro central, menús y palette. En terminales estrechas se muestra el grupo activo conservando el otro. El diff del archivo activo frente a HEAD/index ya está conectado en modo de solo lectura; símbolos e includes/RUN siguen pendientes y M2A permanece abierto.

F1 retiró los campos espejo de cursor/selección/scroll del documento. Los grupos poseen vistas persistentes con ViewId y comparten el documento/undo. Replace-all publica una sola transacción; cerrar una vista preserva otras referencias. F2 ya sustituyó la decodificación completa de navegación/render por proyección compartida y rangos visibles; F3 separa preparación/render, emite diferencias y entrega trabajos internos sin otra tecla. Véanse [avance F3](docs/F3_PRESENTATION_TASKS.md), [avance F1](docs/F1_DOCUMENT_VIEWS_TRANSACTIONS.md), [arquitectura](docs/ARQUITECTURA.md) y [plan](docs/PLAN.md).

En este equipo se preparó Rust 1.85.1 dentro de `work/toolchain`, sin cambiar el PATH del usuario. El wrapper PowerShell también admite un Cargo ya instalado:

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

Abre una carpeta y usa Ctrl+Shift+F para consulta literal sensible a mayúsculas, inclusión/exclusión por globs y resultados progresivos. Enter navega; Esc en el panel inferior cancela. Tras una consulta completa sin avisos, Search / Replace in Project permite revisar sustituciones y confirmar con F9/Ctrl+Enter. Abiertos: edición con undo sin guardar; cerrados: guardado por archivo y conflictos explícitos. Véanse [uso, cuotas y límites](docs/S1_PROJECT_SEARCH.md) y [guardado Windows](docs/SAVE_WINDOWS_CONTENTION.md). En documento activo, Ctrl+F/Ctrl+H ya ofrecen búsqueda acotada, case folding completo Unicode 17, selección/historial y Ctrl+G escalar: [S0](docs/S0_DOCUMENT_SEARCH.md). No se certifican regex, normalización Unicode, GUI ni objetivos globales de rendimiento.

En Windows, abre la raíz del repositorio y usa View / Git: Active File Diff; la primera vez solicita la ruta absoluta de Git para esa sesión. HEAD/index, refresh, cancelación y configuración están en View y palette. La comparación incluye cambios sin guardar (hasta 2 MiB por fuente), muestra identidad/OID y permite navegar hunks sin editar snapshots. Cambiar documento, vista, revisión o encoding exige refresh. [Entrega y límites](docs/GIT_DIFF_DELIVERY.md), [presentación TUI](docs/GIT_DIFF_PRESENTATION.md) y [proveedor Git](docs/GIT_READ_ONLY_PROVIDER.md). El estado de repositorio de solo lectura se describe abajo; no hay watcher; operaciones Git con escritura siguen en M5. La [escena de estudio GUI](docs/M2B_SCENE_STUDY.md) conserva su alcance estático, sin backend elegido ni editor gráfico funcional.

## Política de CI

El CI remoto se reserva para **M5 (hardening)**. Durante M2–M4 GitHub Actions no se dispara automáticamente por push/PR y no debe utilizarse como bucle de depuración de código todavía inestable. El workflow permanece manual y dormido hasta M5; la validación intermedia se hace local cuando el entorno lo permite.

## Varios lenguajes

Perfiles iniciales: **ABL, C#, Rust, JavaScript/JSX, TypeScript/TSX, Python, HTML, CSS,
SQL, XML, JSON, JSON con comentarios, Markdown, TOML, YAML, PowerShell, Shell y texto**.

Actualmente se reconocen los perfiles y se pueden editar sus archivos como bytes.
**Resaltado, parsers, completado semántico, LSP y depuración aún no están implementados.**
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

La licencia de distribución del código nuevo está pendiente de decisión del propietario.
Las condiciones BSD de la parte derivada de Lime se conservan: [LICENSING.md](LICENSING.md).

Consulta [CONTRIBUTING.md](CONTRIBUTING.md) para construir, probar y ampliar el proyecto.


El [estado Git por repositorio](docs/GIT_STATUS_READ_ONLY.md) y su
[entrega versionada](docs/GIT_STATUS_DELIVERY.md) ya están integrados/validados:
261 pruebas completas, fmt/Clippy/release locales. No identidad Document ficticia,
sin escrituras, rutas byte opacas y configuración/filtros explícitos. Presentación
TUI de status integrada: Open Folder con la raíz del repositorio y Tools /
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

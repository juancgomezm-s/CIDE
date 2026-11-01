# Prompt de continuación local — JC-IDE, M2A

Copia este texto al agente de programación que vaya a trabajar directamente en el equipo Windows/Zephyrus. Debe inspeccionar el repositorio antes de modificarlo; este documento es un handoff, no una certificación de que todas las funciones están compiladas.

**Actualización 2026-11-01:** el doble panel de prioridades 1–2 del prompt original ya está implementado, compilado y probado localmente. Conserva el prompt inferior como contexto del alcance original; para el estado actual consulta [M2_TUI_CHECKPOINT.md](M2_TUI_CHECKPOINT.md) y [VALIDACION.md](VALIDACION.md). No rehacer PaneManager ni el renderer. El siguiente incremento es navegación por proveedores (Outline/includes/RUN); Search Plus de proyecto y Git read-only siguen pendientes. M2A permanece abierto y aún requiere revisión manual exhaustiva. En este equipo `scripts/cargo-local.ps1` utiliza el toolchain aislado en `work/toolchain`; ambos se describen en README.

---

Actúa como agente responsable de continuar **JC-IDE**, editor multilenguaje Rust, desde el estado real de mi repositorio privado `https://github.com/juancgomezm-s/JC-IDE.git`, rama `main`. Trabaja en mi entorno Windows local y realiza cambios reales sobre el repositorio.

## Primer paso obligatorio

1. Localiza el clon de JC-IDE. Si no existe, clónalo en la carpeta de proyectos acordada. Haz `git status`, `git fetch` y revisa `git log -5 --oneline`. **No uses `reset --hard`, `clean -fd` ni sobrescribas cambios locales sin consentimiento**.
2. Lee `AGENTS.md`, `README.md`, `docs/PLAN.md`, `docs/TUI_INTERACTION.md`, `docs/EXTENSION_ARCHITECTURE.md`, `docs/M2_TUI_CHECKPOINT.md`, `docs/M2_NAVIGATION_PANES_GIT.md` y `docs/M2_SEARCH_PLUS.md` antes de tocar código.
3. Inspecciona `crates/jc-app/src/panes.rs`, `crates/jc-app/src/workbench/mod.rs`, `crates/jc-app/src/tabs.rs`, `crates/jc-tui/src/render/mod.rs` y `crates/jc-tui/src/app/events.rs`.
4. Comprueba localmente `cargo fmt --all -- --check`, `cargo test -p jc-app`, `cargo check -p jc-tui` y después los tests dirigidos a lo que modifiques. Corrige los errores encontrados, sin fingir resultados y sin disparar GitHub Actions.
5. No reactivar ni ejecutar GitHub Actions en M2–M4; **CI remoto y hardening integral pertenecen a M5**, por decisión D13.

## Qué existe al comenzar

- Core byte-preserving (`jc-core`), codificaciones explícitas (`jc-text`), guardado seguro (`jc-files`).
- `jc-app`: Workbench, CommandRegistry, KeyMap, MenuModel, Tabs, foco, búsqueda y operaciones de edición.
- `jc-tui` inicial: terminal con crossterm, Explorer, búsqueda, menús y panel inferior.
- `jc-languages`: 18 perfiles y delimitadores de comentarios declarativos.
- `jc-progress`: puente de solicitudes/diagnósticos, sin validación de helpers ABL en AVM.
- **Checkpoint nuevo:** `PaneManager` y métodos de Workbench para Primary/Secondary, pin de referencia, historial atrás/adelante y flag de scroll vinculado. Tiene pruebas de modelo y Workbench, pero no se debe asumir compilado.
- **No están hechos:** renderer doble TUI, scroll/cursor/selección completos por vista, resolución inteligente de includes/RUN, autocomplete semántico, DiffModel, motor Git de escritura o merge 3 vías.

## Tu primer objetivo: cerrar el incremento de doble panel de M2A

**Prioridad 1 — validar y terminar el modelo.** Asegura que dos grupos puedan mostrar el mismo TabId y la misma FileSession, que cada grupo conserve foco/cursor/scroll/selección propios, que el anclaje no sea reemplazado al navegar y que atrás/adelante use un historial independiente por grupo. Corrige la reubicación de offsets al editar contenido visible en dos grupos y el cierre de pestañas abiertas simultáneamente. El historial de undo debe seguir siendo uno por documento. Mantén un máximo de dos grupos inicialmente.

**Prioridad 2 — conectar el renderer TUI.** Divide la región central en Primary y Secondary sin duplicar Workbench. Respeta los anchos de terminal; cuando el ancho no alcance, muestra un grupo y permite alternar, sin destruir el otro. Dibuja título de cada grupo, activo, indicador de pin y scroll link; permite Focus Group 1/2. Agrega acciones reales Split Right, One Group, Pin/Unpin, Linked Scroll, Navigate Back/Forward al CommandRegistry/KeyMap/MenuModel/Palette. No registres atajos sin handler funcional. Fallback por Palette para terminales que no transmitan combinaciones. Prueba navegación únicamente por teclado y preserva F6, Ctrl+D, Ctrl+Shift+D, Tab/Shift+Tab, Ctrl+F/H, F7 y Alt+1…9 existentes.

**Prioridad 3 — navegación lateral y símbolos.** Amplía Explorer hacia Outline/Breadcrumb genérico, con resultados tipados. Define un resolver por proveedor: para ABL, detectar con lexer ligero `{customer.i}` y `RUN orders.p` estáticos, resolver usando ruta fuente + PROPATH/perfil de proyecto, mostrar múltiples candidatos y marcar dinámicos/no resueltos. Un `F12` o Enter debe abrir el destino en el grupo no anclado; Alt+F12 Peek y Alt+Left/Right historial. No ejecutar AVM ni BD para navegar; esas integraciones llegarán en M3/M4. No fingir referencias semánticas donde solo hay heurística.

**Prioridad 4 — Git read-only en M2.** Implementa una capa independiente que use Git CLI instalado, sin invocaciones de shell concatenadas, para status/log y diffs worktree/index/HEAD/commit; muestra cambios en sidebar Git y abre diff lado a lado en los dos grupos. Mantén modelos de snapshots inmutables y resultados tipados. El stage/unstage, commit, fetch/pull/push, ramas, merge y resolución BASE/OURS/THEIRS/RESULT quedan planificados para M5, con confirmación y control de conflictos. No hagas push, merge ni modificación de ramas de negocio como efecto de navegar.

**Prioridad 5 — preparar autocomplete y Search Plus.** Define interfaces universales de Symbol/Completion/NavigationTarget en la capa común sin meter reglas ABL en el core. El proveedor ABL aportará keywords, nombres de includes/procedures y luego tablas/campos desde caché local; otros lenguajes podrán aportar LSP opcional. Búsquedas de proyecto deben ser cancelables, acotadas y respetar encodings legacy. No iniciar procesos ni indexación residente por defecto.

## Invariantes

- TUI y GUI comparten Workbench, PaneManager, CommandRegistry y KeyMap.
- Core nunca importa `jc-progress`, una runtime Python ni una extensión específica.
- Guardado/edición preserva los bytes originales, codepages legacy, BOM y EOL. Ninguna modificación silenciosa por caracteres inválidos.
- Una edición multiposición produce una sola transacción lógica de undo.
- No inventar disponibilidad: comandos sin implementación se deshabilitan.
- La integración de plugins externos/Sublime se mantiene **post-M5**; las capacidades de lenguaje internas sí pueden prepararse ahora.
- Sin uso de GitHub Actions hasta M5 salvo mi autorización expresa.

## Entregables

1. Código compilable con validación **local** y pruebas dirigidas que demuestren split/pin/back/forward/restore, apertura lateral, mismo documento en dos grupos y ausencia de corrupción del undo.
2. Un resumen de cambios y limitaciones reales; no marcar M2A cerrada si aún faltan funciones.
3. Actualización de `docs/M2_TUI_CHECKPOINT.md`, `docs/TUI_INTERACTION.md` y el plan cuando el código avance.
4. Commits Git con mensajes claros, sin reescribir mi historial ni pisar modificaciones locales. Al finalizar, explica rama/commit y qué hay que revisar antes de integrar a main.

Comienza inspeccionando y validando el checkpoint existente; no reconstruyas el editor desde cero.

---

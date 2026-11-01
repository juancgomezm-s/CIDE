# M2–M5 — Grupos de edición, navegación y Git integrado

**Estado:** especificación y checkpoint técnico. No implica que la TUI ya dibuje dos paneles, ni que Git o un LSP estén operativos.

## Objetivo de experiencia

JC-IDE debe aproximarse a la experiencia de navegación de Sublime Text con un único `Workbench` byte-preserving: dos grupos simultáneos de edición, referencias anclables, navegación atrás/adelante, explorador izquierdo, símbolos e includes navegables y una experiencia Git integrada para comparar, revisar y resolver conflictos. TUI y GUI comparten comandos, estado, history y servicios de lenguaje. Una extensión aporta semántica, no un segundo editor.

### Superficies

```text
┌──────────────────────────────────────────────────────────────────────┐
│ File Edit Search Navigate Project Git View Build Tools Help         │
├────────────────────┬─────────────────────────┬───────────────────────┤
│ EXPLORER / OUTLINE │ GRUPO 1: customer.p     │ GRUPO 2: customer.i   │
│ Project tree       │ [customer.p] [orders.p] │ [customer.i] [diff]   │
│ Symbols            │ RUN orders.p            │ DEFINE BUFFER ...     │
│ Git changes        │ {customer.i}            │ (pinned reference)    │
│ Commits/branches   │                         │                       │
├────────────────────┴─────────────────────────┴───────────────────────┤
│ Problems / Output / Search / Git changes / Diff hunks                │
├──────────────────────────────────────────────────────────────────────┤
│ Ln:127 Col:8 | ABL | IBM850 | branch | group 1 | reference pinned    │
└──────────────────────────────────────────────────────────────────────┘
```

Este dibujo es objetivo arquitectónico, **no** una captura ni evidencia de interfaz implementada.

## 1. Modelo de grupos: una fuente de verdad

- Máximo inicial: **dos grupos**, Primary y Secondary. Ningún panel duplica `FileSession`, `Document` ni undo/redo.
- Cada grupo conserva un `NavLocation(TabId, byte_offset)`, estado anclado y stacks atrás/adelante **independientes**; los movimientos ordinarios de cursor no llenan el historial de saltos.
- Al abrir Split Right, ambos grupos apuntan inicialmente al mismo documento. Al navegar a otro archivo se reemplaza el grupo activo, salvo que esté anclado; en ese caso se usa el otro grupo.
- Solo un grupo puede estar anclado como referencia para evitar que dos grupos bloqueen la navegación automática. Un salto explícito sigue estando permitido.
- **Pin** significa preservar el documento referenciado frente a navegación automática. **Linked scroll** es otro ajuste independiente: empareja desplazamiento de vistas comparables, no cambia el historial.
- Cerrar el segundo grupo no cierra su documento ni descarta sus cambios. Cerrar una pestaña elimina sus ubicaciones de todos los historiales y evita referencias colgantes.
- Un documento compartido tiene una sola revisión/undo; cada vista debe tener cursor, selección visible, scroll y estado de foco propios. La capa de vistas deberá rebasar offsets al editar delante de otra vista, sin modificar la semántica del buffer.
- La comparación Git no es un documento editable común: los lados de commit/index/HEAD son **snapshots inmutables**; la vista de resultado de merge sí es editable y guarda vía `jc-files`.

**Código de arranque en M2:** `crates/jc-app/src/panes.rs` con modelo Primary/Secondary, anclaje, navegación y linked-scroll flag; `Workbench` expone métodos de grupos. El renderer y la gestión completa de cursor/scroll por vista siguen pendientes. El código nuevo aún no se ha compilado en el entorno de esta sesión.

## 2. Navegación de código (universal y por proveedor)

**Núcleo genérico:** `NavigationTarget` contiene URI/ruta, `DocumentId/TabId` cuando ya está abierto, rango fuente en bytes, línea/columna opcionales, origen de evidencia y confianza. Acciones: Open, Open to Side, Peek, Go to Definition, Find References, Back, Forward, Next/Previous Symbol, Breadcrumb, Outline.

**Resolución por lenguaje:** `jc-languages` aporta perfil/capacidades; los proveedores internos aportan símbolos y resolución:

- **Progress ABL**: reconocer includes como `{customer.i}` y llamados estáticos `RUN orders.p`; resolver mediante carpeta del programa, `PROPATH`, includes y perfiles de proyecto. Mostrar candidatos cuando haya rutas ambiguas y no ejecutar código para navegar.
- Para `RUN VALUE(...)`, `RUN procedure IN handle`, macros/preprocesador y llamados dinámicos, mostrar **no resuelto o aproximado**; no inventar destino seguro.
- Identificar `PROCEDURE`, `FUNCTION`, `METHOD`, `CLASS`, `DO/FOR EACH/CASE` y bloques como Outline/Breadcrumb. La tokenización local se habilita bajo demanda; referencias verificadas mediante XREF/XREF-XML llegan en M3/M4.
- **C# / Rust / Python / JS/TS / SQL**: reutilizar gramáticas locales ligeras, LSP opcional o un provider específico; no insertar reglas ABL en `jc-app`.
- El índice de símbolos se construye por archivo/ámbito y bajo demanda, con límite y cancelación; no hay indexador residente obligado.

**Interacción:** `Enter` en árbol de símbolos abre; en archivo, `F12` intenta definición y `Alt+F12` Peek; si hay múltiples destinos abre un selector TUI/GUI. Los enlaces detectables pueden resaltarse, pero `Ctrl+Click` en GUI y atajos TUI deben llamar el **mismo CommandId**. Cualquier salida de proveedor se valida contra raíz de proyecto y revisión.

## 3. Autocompletado inteligente, sin volver pesado al editor

La respuesta estándar de un provider contiene etiqueta, tipo (`keyword / variable / table / field / procedure / include / file / snippet / symbol`), rango a sustituir, texto/edits, prioridad, origen y compatibilidad de versión.

Orden sugerido:

1. palabras del archivo actual / símbolos locales;
2. keywords/snippets del perfil (cargados bajo demanda);
3. proyecto: nombres de procedimientos y includes;
4. proveedores semánticos: para ABL, buffers/tablas/campos de catálogo local; para C#/Rust, LSP opcional;
5. ayuda agéntica opcional, **nunca** parte obligatoria del autocomplete.

`Ctrl+Space` solicita manualmente, si el terminal reporta la combinación; alternativa Command Palette. No hacer llamadas a la BD o AVM en cada tecla; autocompletar con caché local versionada.

## 4. Git como superficie editorial, no solo botón de push

### Fuentes comparables

- archivo de trabajo vs index (staged);
- index vs HEAD;
- trabajo vs HEAD;
- archivo vs un commit o rama arbitrarios;
- commit A vs commit B, incluidos cambios de nombre y eliminación;
- diff entre dos archivos abiertos, sin Git.

Un `DiffModel` común presenta lado izquierdo/derecho, rutas/SHAs, hunks, contexto, estado de modificación y correspondencia de líneas. Permitir modo unificado y lado a lado; scroll vinculado por hunks (no por números de línea brutos) y panel derecho anclable. Resaltar inserción/borrado/modificación sin convertir snapshots legacy a UTF-8.

### Flujo Git propuesto por etapas

| Etapa | Alcance |
|---|---|
| M2A | Vista Git izquierda, detectar repo, status, cambios y comparación **solo lectura**; no bloquear teclado |
| M2B | GUI de diffs lado a lado usando el mismo DiffModel |
| M3–M4 | Integración de navegación por símbolos/cambios y vista de commits; provider Git genérico sin contaminación de ABL |
| M5 | Staging selectivo, unstaging, commit, fetch, pull, push, ramas, checkout, merge, resolución de conflictos y hardening |
| Posterior | Rebase interactivo y operaciones complejas si se demuestra necesidad |

**Backend inicial recomendado:** Git CLI instalado, invocado bajo demanda con `std::process::Command` y argumentos separados, sin shell ni ejecución automática en segundo plano. Admite repos múltiples por workspace; no guardar claves/credenciales ni escribir secretos en logs. Comparar con libgit2/gitoxide solo si una medición indica necesidad.

### Operaciones Git con confirmación y seguridad

- Status/log/diff/blame son lecturas; pueden cancelarse y tienen límites de salida.
- Stage/unstage por archivo o hunk debe trabajar sobre el índice esperado y verificar cambios entre previsualización y aplicación.
- Commit requiere revisar exactamente qué está staged y mensaje explícito.
- Fetch, pull, push, checkout, merge y cambios de rama se hacen **solo por orden del usuario**. Antes, comprobar working tree sucio, ramas divergentes y conflictos.
- No ejecutar reset --hard, clean -fd, force push ni stash destructivo automáticamente.
- Fetch/pull/push gestionan credenciales por Git Credential Manager/SSH del usuario; la TUI no pide guardar tokens en el proyecto.
- Git hooks son código externo: marcar el posible riesgo y no introducir ejecución encubierta a través de comandos del editor.
- Multi-repo: cada acción tiene un `RepositoryId` explícito y no asume que todos los directorios pertenecen al mismo repositorio.

## 5. Resolución de conflictos — editor de tres vías

El merge no es un diff binario con botones ambiguos. Se necesitan **BASE / OURS / THEIRS**, una vista RESULT editable y un estado por conflicto:

- aceptar ours, theirs, ambos, o editar manualmente;
- saltar siguiente/anterior conflicto;
- visualizar cambios de contexto y diferencias con respecto a BASE;
- marcar un conflicto resuelto solamente si la vista RESULT es válida y el usuario lo confirma;
- guardar resultado sin perder bytes/EOL, verificar que los archivos no cambiaron externamente;
- actualizar el index únicamente tras confirmación; advertir si quedan conflictos antes de commit.

Se deben probar conflictos de renombre/eliminación, EOL mixtos, fuentes legacy y conflictos que afectan al mismo byte. No se permitirá auto-merge de un archivo cuyo encoding no pueda interpretarse con seguridad.

## 6. Tabla de teclado propuesta

**Reservas, no todas implementadas.** Las opciones también estarán en Command Palette, porque algunas terminales no entregan ciertas combinaciones.

| Acción | Atajo propuesto |
|---|---|
| Split Right (dos columnas) | Alt+Shift+2 |
| One Group | Alt+Shift+1 |
| Focus grupo 1 / grupo 2 | Ctrl+1 / Ctrl+2 (fallback Palette) |
| Pin/Unpin Reference | Ctrl+Alt+P |
| Link/Unlink Scroll | Ctrl+Alt+L |
| Navigate Back / Forward | Alt+Left / Alt+Right |
| Quick Open File | Ctrl+P |
| Go to Symbol | Ctrl+R |
| Go to Line | Ctrl+G |
| Go to Definition | F12 |
| Peek Definition | Alt+F12 |
| Find References | Shift+F12 |
| Trigger Completion | Ctrl+Space |
| Git Sidebar | Ctrl+Shift+G (ya reservado) |
| Command Palette | Ctrl+Shift+P (existente) |
| Editor Area Cycle | F6 / Shift+F6 (existente) |

**Invariantes de atajos:** Ctrl+D duplicar, Ctrl+Shift+D borrar línea, Tab/Shift+Tab sangría, F7 Check Syntax ABL, Ctrl+F/H buscar/reemplazar y Alt+1…9 pestañas siguen intactos. Teclas que colisionen con sistema operativo, terminal o IME nunca son único acceso. La futura acción Open to Side puede ofrecerse desde el árbol por `Ctrl+Enter` cuando el terminal distinga el evento.

## 7. Diseño responsive TUI

- Pantalla amplia: sidebar + dos grupos + panel inferior.
- Media: sidebar colapsable, dos grupos si hay ancho mínimo medido; si no, una sola vista visible y el otro grupo conservado.
- Estrecha: mostrar un solo grupo a la vez; comandos de grupo siguen operativos.
- No prometer valores de ancho exactos antes de medir caracteres Unicode, scrollbars y menús. No sacrificar la legibilidad para forzar dos paneles.
- UI no tiene su propio manager de documentos; recibe `PaneManager`, `Workbench` y eventos centrales.

## 8. Criterios para cerrar las capacidades

### M2A: grupos y navegación

- Abrir dos archivos en grupos distintos sin duplicar `FileSession`.
- Anclar una referencia, navegar desde el otro grupo y regresar/avanzar por historial independiente.
- Guardar/undo de archivo compartido es coherente aunque haya dos vistas; el cursor/scroll individual no se mezcla.
- Cambiar tamaños/ocultar sidebar no pierde grupo ni contenido.
- Todas las acciones son accesibles sin mouse; los shortcuts no colisionan con `KeyMap`.
- Explorer/Outline a código abre destino exacto; para ABL ambiguo muestra candidatos, no ruta inventada.
- Comparación de archivos y de working tree vs HEAD solo lectura en M2A.

### M5: Git completo

- Tests de status, stage/unstage por hunk, commits, fetch/pull/push con errores controlados.
- Preflight de repos sucios, ramas divergentes y conflictos.
- Merge con vistas BASE/OURS/THEIRS/RESULT, preservación de codificación y confirmación antes de index.
- Ausencia de procesos Git residentes sin necesidad; seguridad de credenciales y repositorios no confiables.
- CI remoto y hardening transversal **solo en M5**, por D13.

## 9. Deuda técnica reconocida

- El `Workbench` inicial tiene un cursor por documento. El nuevo `PaneManager` registra ubicaciones por grupo, pero **no** proporciona todavía scroll/selección independientes completos ni ajustes de offsets después de ediciones.
- El enum fijo `CommandId` aún no incluye acciones de navegación y Git; registrar sus comandos cuando exista el ejecutor real, en lugar de mostrar placeholders engañosos.
- Aún no se implementan UI dividida, resolver de includes/RUN, autocompletado semántico, DiffModel, Git write flows ni merge editor.
- Los artefactos nuevos de M2 no tienen compilación verificada; ejecutar validación **local dirigida** al continuar desde el equipo. GitHub Actions permanece dormido hasta M5.

Consulte también [TUI_INTERACTION.md](TUI_INTERACTION.md), [EXTENSION_ARCHITECTURE.md](EXTENSION_ARCHITECTURE.md), [M2_SEARCH_PLUS.md](M2_SEARCH_PLUS.md) y [PLAN.md](PLAN.md).

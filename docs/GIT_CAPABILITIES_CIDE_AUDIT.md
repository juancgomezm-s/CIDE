# Auditoría de capacidades Git de CIDE y expansión prevista

**Decisión posterior de Juan:** el motor será un [núcleo compartido independiente](GIT_SHARED_CORE.md)
para CIDE, OmniCore y OmniCoder. G1–G3 describen la expansión de consulta; no todo
ese alcance es requisito para cerrar M2. Las referencias M2 siguientes indican
operaciones de lectura y su orden, no un bloqueo de cierre por paridad completa.

Fecha: 2026-11-05. Auditoría estática del código local, sin ejecutar pruebas,
consultas de red ni operaciones Git. Complementa el estudio REA de Sublime Merge
realizado por el integrador en [Estudio de Sublime Merge](SUBLIME_MERGE_REA_STUDY.md);
este documento no atribuye hallazgos de REA propios. La configuración posterior
de Ghidra/JDK y las trazas nativas del integrador no cambian esta auditoría de
capacidades CIDE ni prueban que CIDE ya implemente las funciones estudiadas.
Las capturas del usuario definen expectativas de interfaz, no una autorización
para ejecutar sus acciones sobre repositorios personales.

## Qué está construido

| Herramienta | CIDE GUI actual | CIDE TUI actual | Diferencia por construir |
|---|---|---|---|
| Estado local | Lista tipada: índice, trabajo, no rastreados y conflictos; resumen inferior y actualización | Lista tipada acotada con selección, desplazamiento y actualización | Filtros por estado/ruta; apertura y comparación desde una fila TUI |
| Rama actual | Nombre en encabezado del espacio Git | El proveedor entrega metadatos, pero la vista de estado no compone un explorador de ramas | Lista de ramas locales/remotas, etiquetas y selección de referencia |
| Historial | Hasta 24 commits por primer padre, autor, resumen, hash y padres | Sin explorador de historial | DAG paginado, fusiones, etiquetas, búsqueda y navegación de revisiones |
| Comparación | Documento activo, incluidos cambios sin guardar, frente a HEAD o índice | Mismo contrato de documento activo frente a HEAD/índice | HEAD frente a índice, índice frente a disco y revisiones arbitrarias como pares explícitos |
| Visualización diff | Filas unificadas rojo/verde, líneas anteriores/nuevas y contexto del bloque seleccionado | Bloques y filas acotados; navegación a fuente tras validar identidad | Vista paralela, contexto ampliable, cambios intralínea, resaltado de sintaxis y navegación horizontal |
| Apertura desde estado | La fila resuelve ruta confinada y reutiliza documento abierto | El estado aún no abre archivos | Fuentes históricas inmutables y archivos nuevos/eliminados con un lado ausente explícito |
| Distribución de paneles | Tres columnas a partir de 760 píxeles de diseño; debajo prioriza archivos/comparación | Panel lateral Git y panel inferior Diff | Selector 1/2/3 columnas, archivos izquierda/derecha, divisores y preferencias persistentes |
| Historial de archivo / autor por línea | No implementado | No implementado | Historial por ruta y anotaciones por línea, con límites explícitos sobre renombrados |
| Exportar parche / copiar datos Git | Sin flujo específico | Sin flujo específico | Exportación elegida por usuario, copiar hash/ruta y abrir carpeta/editor |
| Preparar, retirar del índice y commit | No implementado | No implementado | Motor de escritura transaccional de M5, revisión de alcance y recuperación |
| Ramas y etiquetas: crear/renombrar/borrar/cambiar | No implementado | No implementado | Comandos tipados M5; no confundir mostrar una referencia con cambiarla |
| Revertir, cherry-pick, reset, rebase y merge | No implementado | No implementado | Secuenciador M5 y resolución de conflictos de tres vías |
| Stash, submódulos y worktrees | Sin gestión; estado rechaza gitlinks; apertura reconoce gitfile/commondir | Igual motor | Gestión y pruebas específicas; reconocer un worktree no equivale a administrarlo |
| Fetch/pull/push/clone y credenciales | No implementado | No implementado | Transporte embebido, credenciales, progreso y recuperación M5 |

Una fila con estado de conflicto no implica que exista un resolvedor. La GUI puede
rechazar comparar un archivo eliminado o sin blob base; actualmente no sintetiza
la fuente vacía necesaria para mostrar una adición/eliminación completa.
Tampoco debe llamarse «diff preparado» al par HEAD/documento: el documento puede
diferir tanto del índice como del disco.

## Evidencia de fuente

- `crates/jc-git/Cargo.toml`: gix=0.90.0, sin features por defecto; solo status,
  revision, sha1 y sha256 seleccionadas directamente.
- `crates/jc-git/src/lib.rs`: `open`, `read_blob`, `read_status`,
  `repository_info`, `RepositoryInfo`, `CommitInfo` y `Limits`. Los únicos puntos
  públicos de operación del proveedor son lectura de blob y estado. `INFO_COMMITS`
  es 24 y `INFO_BYTES` es 24 KiB. Se comprueba objeto/OID y se rechazan formatos
  incompatibles, filtros externos y gitlinks en la ruta actual.
- `crates/jc-app/src/git_read.rs`, `git_status.rs`, `git_status_tasks.rs` y
  `git_diff.rs`: adaptador embebido, snapshots y controladores versionados.
  La aceptación de resultados no pertenece al render.
- `crates/jc-app/src/command.rs` y `menu.rs`: hay comandos de lectura Git,
  sin comandos stage/commit/push operativos.
- `crates/jc-gui/src/windows/git_workspace.rs`: `Layout::new`,
  `prepare_git_workspace`, `git_workspace_click`, `compare_git_status_file` y
  `git_workspace_wheel` forman la interfaz actual; `Intent` no incluye escrituras.
  `source_text` produce texto acotado, no tokens de sintaxis para el diff.
- `crates/jc-tui/src/app/git_status.rs` y `render/git_status.rs`: estado de lista
  y composición; el módulo declara expresamente ausencia de acción ruta→archivo.
  `app/git_diff.rs` solicita y presenta el diff del documento activo.

Las evidencias de ejecución previas están en `M2_GIT_WORKSPACE_NATIVE.md` y
`GIT_EMBEDDED_ENGINE.md`; esta auditoría no amplía sus cifras ni sus garantías.

## Arquitectura para ampliar sin otro motor de edición

`jc-git` debe seguir siendo el proveedor transversal Rust. Objetos Git son fuentes
inmutables: una revisión histórica no sustituye el contenido editable de un
DocumentId. `jc-app` organiza consultas y comandos versionados, GUI/TUI consumen
las mismas capacidades y cada frontend decide su composición visual.

Separar una consulta de repositorio (refs, DAG, archivos de commit, historial de
ruta) de una consulta de comparación (dos fuentes con identidad). Las fuentes
necesitan distinguir commit/árbol/blob, índice, disco, buffer y ausencia del lado;
un blob ausente por adición no equivale a un error de lectura o un objeto corrupto.
Cada par debe fijar su identidad antes de producir filas. Cambiar raíz, referencia,
revisión, índice o documento invalida entregas y clics anteriores.

El recorrido de historial debe ser paginado y cancelable, con un presupuesto
común de nodos/aristas/rutas/objetos y marcadores de continuidad. El grafo muestra
fusiones reales; cortar una página no debe fingir que su último nodo es raíz.
No se propone un indexador permanente ni escanear el repositorio al pintar.

## Bloques amplios propuestos

1. **G1, consulta histórica compartida (M2, solo lectura).** Referencias, commits
   paginados, árbol y lista de archivos de una revisión, comparación con su padre
   y selección explícita de padre en fusiones. Integrar archivos nuevos/eliminados,
   búsqueda acotada de historial y apertura lateral inmutable. Cerrar con fixtures
   de fusiones, HEAD sin commit, ramas divergentes, raíz, shallow y resultados
   obsoletos; comprobar fuentes/metadatos intactos.
2. **G2, espacio Git de consulta (M2).** Layout persistente 1/2/3 columnas,
   archivos izquierda/derecha, divisores, barras, pestañas Resumen/archivo,
   árbol Cambios/Archivos, diff unificado/paralelo y menús contextuales pequeños.
   Acciones visibles solo cuando exista capacidad, no botones decorativos.
3. **G3, herramientas de consulta (M2).** Historial por ruta, anotaciones por línea,
   búsqueda y exportar parche. Fijar límites de seguimiento de renombrados y
   confirmar que el parche exportado corresponde al par y a las opciones mostradas.
4. **G4, escrituras locales seguras (M5).** Preparar/retirar archivos y bloques,
   commit/amend, refs y restore. Preflight del índice, HEAD, fingerprints de disco
   y buffers sucios; exclusión por repositorio, publicación por locks/rename y
   registro recuperable. Un cambio sobre varios archivos del worktree no es
   atómico por tener un índice atómico: debe admitir recuperación y estado parcial
   explícito. El undo del editor no se presenta como undo de Git.
5. **G5, integración y secuenciación (M5).** Merge, revert, cherry-pick, reset,
   rebase interactivo y stash; resolver base/nuestro/suyo/resultado, poder cancelar,
   continuar o recuperar la operación. Previsualización del alcance, identidad
   esperada y protección de cambios sin guardar antes de modificar fuentes.
6. **G6, remotos y repositorios ampliados (M5).** Fetch/clone/push, pull como
   fetch+integración explícita, upstreams, credenciales y rechazo no fast-forward;
   después submódulos/worktrees y capacidades opcionales de firma/LFS. Cada una
   necesita evidencia propia y una política de dependencias/procesos explícita.

No se adelantan mutaciones a M2 por ampliar la referencia visual. La petición de
mayoría de herramientas queda cubierta por el plan completo; la implementación
sigue las puertas vigentes y se valida al cerrar cada bloque.

## Qué permite afirmar el gix local y qué falta

Se inspeccionó la copia de gix 0.90.0 del caché del workspace. Existen primitivas
como `Repository::write_object`/`commit_as` en `repository/object.rs`,
`edit_references` en `repository/reference.rs` y `rev_walk` en
`repository/revision.rs`. Su existencia permite evaluar adaptaciones, pero no
prueba que stage parcial, checkout seguro, rebase, stash o push estén resueltos.

Su `Cargo.toml` declara features opcionales `blame`, `merge`,
`worktree-mutation`, `credentials` y transportes de red. No están habilitadas por
la selección actual de CIDE. No se descargó ni compiló ninguna de ellas en esta
auditoría. Debe revisarse cobertura real de cada operación antes de elegir qué
parte adaptar y qué parte construir en Rust propio. No habilitar `extras` por
comodidad ni prometer paridad porque exista una feature con nombre parecido.

Para HTTPS/SSH embebido hay que evaluar transporte, TLS y autenticación sin
reintroducir git.exe ni depender implícitamente de ssh.exe/auxiliares. Algunas
rutas genéricas de Git pueden invocar procesos (credenciales, filtros o firma);
la política actual los excluye. Cualquier alternativa Rust requiere fijar versión,
coste de dependencias/licencias y pruebas propias. Credenciales no van en argv,
logs, estado recuperable del editor ni capturas. No se presupone que el soporte
actual de SHA-256 sea aceptación completa de todos los repositorios SHA-256.

## Reservas existentes

La GUI y la TUI muestran **Reserva stash@{n}** en **Git → Grafo → Referencias**.
**Intro** abre el commit de la reserva por su identificador y permite revisar
los archivos y compararlos contra el padre elegido. Se muestran las 24 más
recientes a partir de un reflog de hasta 64 KiB. Si la lista queda truncada o el
reflog falta, es inválido o no coincide con la referencia, el grafo señala el
historial parcial. Consultar reservas conserva intactos el índice, los archivos
y el reflog.

Quedan pendientes guardar, aplicar, extraer y eliminar reservas, además de
fetch, pull, push, clone, configurar la rama de seguimiento, submódulos y
worktrees. La versión fijada de gix tiene primitivas de negociación y fetch,
pero CIDE todavía debe integrar un transporte autenticado y el protocolo de
publicación. El transporte de archivos de gix puede invocar git-upload-pack,
una ruta que queda fuera del requisito de motor embebido.

## Propuesta para la TUI

Compartir consultas y acciones, no comprimir cuatro columnas en una terminal.
Proponer pestañas **Cambios · Historial · Referencias** y un panel de comparación
inferior o a la derecha según ancho. Tab/Shift+Tab cambian panel, flechas navegan,
Intro abre detalle, Ctrl+Intro abre al lado, Esc vuelve; mostrar la ayuda junto a
la acción y evitar nuevos atajos globales hasta verificar colisiones del keymap.
En terminal ancha puede mostrarse un grafo de celdas estrecho, con alternativa
ASCII y continuidad marcada. En terminal pequeña predomina una lista y el detalle
ocupa el área al abrirlo. Ratón y teclado consumen los mismos destinos versionados.

Un menú contextual o paleta dependiente de selección ofrece las herramientas
reales de archivo/commit/rama. Las acciones M5 deben pedir una intención explícita
y mostrar el alcance; nunca activar stage o checkout como efecto de navegar.
La propuesta TUI todavía no está implementada y no añade evidencia de ConPTY.


## Contrato propuesto para G1: dos fuentes con identidad

Esta sección es especificación, no API implementada. Debe sustituir gradualmente
el contrato de diff activo actual, retirando la ruta reemplazada; no añade otro
motor ni otra copia editable. El `OwnedDiffModel` existente puede seguir haciendo
el alineamiento byte a byte una vez resueltas las fuentes.

### Solicitud y tipos

Una `ComparisonRequest` contiene `RepositoryKey`, `RequestGeneration`, dos
`SourceSelector`, `DiffLimits` y una intención de seguimiento (`Pinned` o
`FollowCurrent`). `RepositoryKey` debe incluir raíz/worktree, gitdir y commondir
canónicos más identidad/época de apertura; no basta comparar un nombre de carpeta.
`GitPath` conserva bytes de ruta Git sin conversión con pérdida; mostrar una ruta
escapada no la habilita para apertura Windows. `ObjectId` conserva algoritmo y
bytes de hash; no mezcla SHA-1/SHA-256 ni usa el hash cero como «vacío».

Selectores propuestos:

| Selector | Precondición pedida | Identidad del resultado aceptable |
|---|---|---|
| `Buffer(document_id, revision, encoding_epoch)` | Documento existente y revisión/codec esperados; ruta opcional | Identidad de documento, revisión, codec y snapshot acotado; la vista es solo destino de navegación |
| `Index(path, stage, expected_index)` | Token del índice fijado en consulta; stage explícito | Token de índice, ruta, stage, modo y OID exacto |
| `Worktree(path, expected_disk)` | Ruta confinada y fingerprint/identidad de archivo esperados | Handle/identidad y fingerprint de disco revalidados antes/después de lectura acotada |
| `Commit(commit_oid, path)` | Commit explícito existente del mismo repositorio | Commit, árbol, entrada, modo y blob OID verificado |
| `Head(path, expected_head)` | Alias de consulta, no identidad de contenido | Se resuelve una sola vez a commit/árbol o HEAD sin commit; queda pinneado |

No se propone aceptar `Missing` libremente como un selector que convierta todo
fallo de lectura en vacío. El resolvedor produce un `ResolvedSource` que distingue
`Present` de `Absent`. Una ausencia necesita prueba de contenedor y ruta:
`TreeEntryAbsent(tree_oid, path)`, `IndexEntryAbsent(index_token, path, stage)`,
`WorktreeEntryAbsent(directory_identity, path, checked_epoch)` o
`UnbornHead(expected_ref)`. En el último caso no se inventa un commit: la fuente
conceptual es el árbol vacío. Un objeto anunciado por una entrada pero ausente,
corrupto o con hash incorrecto es error, nunca `Absent`.

Un archivo presente de longitud cero sigue siendo `Present`, distinto de una
ruta inexistente. Para el alineamiento ambos aportan cero bytes, pero acciones,
etiquetas y futuras propuestas de escritura deben conservar esa distinción.
Conflictos del índice no se reducen silenciosamente a stage 0: la solicitud normal
rechaza `Unmerged`; el stage 1/2/3 solo se lee si una operación de consulta lo pide
expresamente y tiene capacidad para presentarlo.

Cada `ResolvedSource` lleva identidad, modo/tipo, longitud, provenance del codec,
bytes inmutables y condiciones de vigencia. Los bytes Git se conservan crudos;
el codec solo afecta a presentación. Binarios y codificación no resuelta pueden
admitirse como lectura exacta con presentación limitada explícita, nunca como
texto convertido con pérdida. Los tipos no admitidos (gitlink/symlink según la
capacidad vigente) fallan antes de compararlos como archivos regulares.

### Captura, publicación y vigencia

1. Resolver identidad de repositorio y aliases de referencias una vez. En una
   solicitud de estado, HEAD/árbol usados por lista y detalles deben ser los mismos.
2. Capturar el token del índice antes de usar sus entradas. Este token identifica
   el archivo de índice completo leído (huella/hash y metadatos de archivo), no
   solo ruta/blob/modo: el `GitBlobIdentity` actual no ofrece ese CAS completo.
3. Leer ambos lados fuera de input/render. Buffer usa el snapshot F3 acotado;
   disco se abre confinado por handle, con verificación antes/después; objetos
   se verifican por OID. Si un lado cambia durante su captura, rechazar el par.
4. Revalidar las precondiciones mutables antes de publicar. Esto entrega dos
   snapshots fijados y sus observaciones, no una instantánea atómica universal
   del filesystem: las modificaciones externas posteriores siguen siendo posibles.
5. Solo `poll_tasks` acepta completions con generación/época y selección actuales.
   La pintura no resuelve fuentes, lee disco, consume resultados ni inicia trabajo.
   Una navegación desde buffer revalida documento/revisión/codec/vista; una vista
   histórica navega únicamente dentro de su fuente inmutable.

`Pinned` conserva OIDs/snapshots elegidos aunque se mueva HEAD después; su etiqueta
explica qué revisión se está viendo. `FollowCurrent` se invalida al observar un
cambio de HEAD/índice/disco/documento y pide actualización. Ninguno implica un
watcher permanente. Separar vigencia de contenido (el OID sigue siendo exacto)
de vigencia de intención (la selección o contexto que solicitó ese contenido).

### Pares iniciales y límites

| Acción | Lado anterior | Lado nuevo |
|---|---|---|
| Cambios preparados | Commit fijado de HEAD / ausencia por HEAD sin commit | Índice stage 0 / ausencia de entrada |
| Cambios guardados de trabajo | Índice stage 0 / ausencia de entrada | Disco / ausencia de archivo |
| Cambios sin guardar | Snapshot de disco o índice elegido explícitamente | Buffer con revisión/codec fijados |
| Revisión histórica | Padre elegido / ausencia de padre en commit raíz | Commit elegido |
| Dos revisiones | Commit A y ruta A | Commit B y ruta B |

En fusiones debe elegirse qué padre se compara; no usar el primer padre sin
indicarlo. No detectar renombrados implícitamente por igualdad de nombres; las
rutas A/B son explícitas o proceden de un detector con contrato posterior.

Mantener como punto inicial 2 MiB por fuente y cuota combinada de fuentes de
4 MiB, además de los límites de líneas/trabajo/metadatos/bloques del diff vigente.
Son presupuestos lógicos y configurables de admisión, no garantías de RSS. Cualquier
aumento debe respetar el menor límite del lector, snapshots F3, modelo y reserva de
artefactos; no se cambia un límite aislado para aparentar soporte de archivos GB.
Compartir `Arc` cuando dos lados resuelvan el mismo snapshot, contando memoria
retenida una vez; no clonar Document, FileSession ni undo.

G1 conserva una comparación en vuelo y una intención pendiente con metadatos,
cancelación cooperativa y reserva de artefacto hasta aceptación/descarte. La
identidad completa del par acompaña cada bloque/target y futura exportación.
El resultado no propone ni aplica ediciones. Este contrato permite después G2/G3
sin que seleccionar una revisión abra o cambie documentos editables.

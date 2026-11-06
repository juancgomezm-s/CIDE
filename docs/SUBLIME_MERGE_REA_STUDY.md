# Estudio de Sublime Merge para CIDE

**Decisión posterior de Juan:** el motor será un [núcleo compartido independiente](GIT_SHARED_CORE.md)
para CIDE, OmniCore y OmniCoder. G1–G3 describen la expansión de consulta; no todo
ese alcance es requisito para cerrar M2. Las referencias M2 siguientes indican
operaciones de lectura y su orden, no un bloqueo de cierre por paridad completa.

Estado: investigación de artefactos distribuidos y diseño; no es implementación
de las capacidades que faltan. Solicitud de Juan del 5 de noviembre de 2026.
Complementa [la auditoría del motor actual](GIT_CAPABILITIES_CIDE_AUDIT.md).

## Procedencia y alcance

Se usó REA 6.8.0 sobre la instalación local `C:/Program Files/Sublime Merge`.
El ejecutable es PE x86-64, 8.218.232 bytes, SHA-256
`f02ffd91ef0fdb7983c27b237888c433bd6ba9f420e57752a41755eeda08387e`.
El changelog local comienza por Build 2132; eso no demuestra por sí solo la
versión del ejecutable ni que coincida con la instalación de las capturas.

REA inventarió y extrajo `Default.sublime-package` y `Theme - Merge.sublime-package`
y también inventarió `Default - Merge.sublime-package`. La extracción de los 126
archivos de Default fue contrastada con sus hashes de REA. Una lectura posterior
de los recursos extraídos analizó 90 archivos JSON con comentarios, sin errores:
60 menús y mapas/configuraciones aportan 1.642 declaraciones de comando y 362
identificadores distintos. Esos totales incluyen edición genérica, alias, comandos
internos y repetidos por plataforma: no son 362 herramientas Git independientes.

Los menús demuestran enlaces declarados entre acciones, argumentos y contextos.
No prueban que cada acción esté disponible o funcione en cualquier repositorio.
Las capturas de Juan aportan evidencia visual de distribuciones, grafo, resumen,
archivos, comparación paralela y menús contextuales. No se ejecutaron esas acciones
sobre los repositorios personales que aparecen en ellas.

La inspección PE de REA encontró 14 DLL importadas y 402 entradas, sin delay imports.
Entre ellas están USER32, GDI32, OPENGL32, IMM32 y DWM. Una importación no demuestra
su uso en una ruta concreta ni permite deducir la estrategia de render o su fluidez.
La instalación incluye una distribución Git y recursos sobre selección de binario;
CIDE mantiene su requisito propio de motor Rust embebido sin git.exe.

La primera búsqueda nativa falló por proveedor no configurado. Juan autorizó
resolverlo: se prepararon Ghidra 12.1.4 y JDK 21.0.13+9, con hashes oficiales,
en `C:/Users/juanc/Documents/Codex/rea-runtime`. El diagnóstico de REA reconoció
ambos y verificó los controles nativos de Windows. El resultado de la prueba real
y cualquier evidencia de decompilación se registran en la nota de configuración;
no deben confundirse con las observaciones de recursos de esta sección.

Los paquetes extraídos, temas, imágenes y código de terceros se mantienen fuera
de las fuentes y del paquete distribuible de CIDE. Este trabajo deriva requisitos
y contratos para una implementación propia; no añade recursos propietarios a CIDE.

## Trazas y decompilación nativa terminadas

Con el plazo ampliado, la misma sesión Ghidra completó tres trazas y tres
consultas de pseudocódigo. Exportó seis registros REA y cerró su base efímera.
El SHA-256 del ejecutable original se volvió a verificar y permaneció igual.
No se ejecutó Sublime Merge durante este análisis estático.

| Consulta | Hallazgo nativo | Función decompilada | Qué permite afirmar |
|---|---|---|---|
| `side_bar_layout` | Una cadena, 11 referencias | `0x140056bb8` | La opción y el valor `columns` aparecen en pseudocódigo; concuerdan con el selector declarado en recursos |
| `stage_file` | Cuatro coincidencias, cuatro referencias | `0x1400d296b` | La búsqueda por subcadena también devuelve `unstage_file`; el pseudocódigo asigna a un botón de archivo una acción con parámetro de ruta |
| `blame` | 16 coincidencias, 14 referencias | `0x1400c4d24` | Hay cadenas/RTTI de comando, constructor, control, gutter y operación; la función recuperada advierte eliminación de bloques inalcanzables |

La consulta de preparación no demuestra el algoritmo de stage: la función
seleccionada contiene el enlace de **retirar del índice**, no una implementación
completa de escritura del índice. Tampoco se recuperó el algoritmo completo de
atribución por línea a partir de la traza `blame`. El pseudocódigo tiene tipos
incompletos y nombres automáticos `FUN_...`, y la función de layout es extensa;
no se interpreta como arquitectura original exacta ni se copia al editor.

Evidencia nativa: `work/rea-merge-native-long-full-evidence.json`, SHA-256 y
registros individuales en el resumen portable. El primer intento de traza de
330 segundos y su cierre se preservan aparte; no se ocultó el timeout ni se
presentó su salida parcial como análisis completo. No se deshabilitaron analizadores.

## Herramientas identificadas

| Familia | Evidencia distribuida observada | Objetivo en CIDE |
|---|---|---|
| Distribución | `Side Bar Layout Options.sublime-menu` | Una columna con pestañas; dos con archivos a izquierda o derecha; tres columnas |
| Comparación | Acciones `toggle_inline_diff`, `toggle_ignore_diff_whitespace` y recursos Diff | Unificada/paralela, contexto ampliable, navegación por bloques y marcas intralínea |
| Referencias | Menús Branch, Remote Branch, Tag y Remote Tag | Ramas locales/remotas, etiquetas, filtros y navegación sin cambiar de rama |
| Historia | Commit, Commit Tree, Author, Date y Visibility | Grafo paginado, padres/hijos, resumen, árbol y selección de revisión |
| Búsqueda | Recursos de búsqueda por autor, rama, ruta, archivo, contenido y fecha | Un campo de búsqueda con filtros sugeridos; ejecución acotada y cancelable |
| Archivo | `File.sublime-menu` y `Commit File.sublime-menu` | Historia de archivo, autor por línea, abrir al lado, carpeta, copiar ruta y parche |
| Preparación | Menús Modified, Untracked, Index, Unmerged y Hunk | Preparar/retirar archivos, selección o bloques; distinguir índice, disco y buffer |
| Commit | Commit Message, Advanced Commit y acciones de edición | Commit/amend, mensaje, autor y revisión del alcance |
| Integración | Revert, cherry-pick, merge y edición de commits | Revertir y aplicar commits/archivos/bloques, merge y resolvedor de tres vías |
| Reescritura | Reset, rebase, squash, fixup, mover y descartar commits | Secuenciador visible con continuar, cancelar y recuperación |
| Trabajo temporal | Stash y Stash Section | Crear, inspeccionar, aplicar y recuperar trabajo temporal |
| Remotos | Remote y acciones fetch/pull/push/clone/upstream | Transporte embebido, progreso, credenciales y errores accionables |
| Repositorios ampliados | Submodule, Worktree y acciones LFS/Git Flow | Soporte por capacidades; no declarar implementados por mostrar un menú |
| Recuperación Git | `git_undo` y `git_redo` declarados en Repository | Diseñar historial propio de operaciones recuperables, distinto del undo del editor |

Se incluyen deliberadamente las herramientas de los menús de archivo y commit de
las capturas; no se limita la expansión al aspecto del grafo. LFS, firmas y Git Flow
son incrementos posteriores con alcance y dependencias propios. Las declaraciones
no aportan por sí solas los algoritmos de esas operaciones.

## GUI propuesta

La GUI tendrá un espacio Git propio dentro de CIDE, con título y menús del tema
global, fuente pequeña uniforme, divisores arrastrables y barras discretas.
Conserva `#2E3440`; la referencia guía organización y comportamiento.

1. **Referencias**: ramas, remotos, etiquetas y trabajo temporal, con contadores
   y filtro. Seleccionar filtra o navega; cambiar de rama es otra acción explícita.
2. **Historial**: grafo y lista sincronizados, búsqueda, etiquetas de referencias,
   selección de uno o dos commits y marcadores de continuidad de página.
3. **Archivos**: alternancia Cambios/Árbol, filtros, estadísticas y selección
   independiente del documento abierto. Los archivos nuevos o eliminados tienen
   un lado ausente tipado, no un error convertido indiscriminadamente en vacío.
4. **Detalle**: pestañas Resumen y archivos, autor/fecha/padres/estadísticas,
   diff plegable por archivo/bloque y modo unificado o paralelo con scroll enlazado.

Las cuatro distribuciones reorganizan estos componentes sin duplicar consultas,
fuentes ni selecciones. En modo estrecho las secciones pasan a pestañas; no se
recortan columnas hasta volverlas inutilizables. Navegar en una revisión histórica
no abre ni cambia automáticamente el archivo editable. Abrir al lado usa una fuente
histórica inmutable; el buffer actual conserva revisión, caret, selección y undo.

Dos elecciones visibles bastan para empezar: **Distribución** y **Comparación**.
Recordar anchos, secciones plegadas y última sección por repositorio es estado de
interfaz, no una nueva pantalla de configuración extensa. Los filtros se sugieren
en el campo de búsqueda. La configuración del tema sigue siendo común al editor.

Menús contextuales pequeños según archivo, commit, rama o bloque. La paleta reúne
las acciones menos frecuentes. Cada acción declara capacidad requerida, contexto,
efecto y precondiciones. Un elemento solo se conecta si existe un handler real;
las funciones futuras se describen en el plan, no como botones decorativos.

## TUI propuesta

Pestañas principales **Cambios · Historial · Referencias**. En una terminal normal
se ve la lista activa y el detalle; en una estrecha se abre el detalle ocupando el
área principal. En una ancha puede colocarse el diff al lado y mostrar un grafo
compacto, con alternativa ASCII y continuidad visible. No se comprime el layout
de cuatro zonas de la GUI dentro de 80 columnas.

Tab/Mayús+Tab cambian panel; flechas navegan; Intro abre detalle; Ctrl+Intro abre
al lado; Esc regresa. Son propuestas que deberán comprobarse contra el keymap real
antes de registrar atajos. Ayuda contextual visible, ratón, barras y menús con
mnemónicos subrayados. El motor y los comandos son compartidos; la composición
de terminal respeta celdas, grafemas, cuotas y clips existentes.

## Orden de construcción y cierre por bloques

| Bloque | Resultado verificable | Cierre local |
|---|---|---|
| G1, consulta compartida | Referencias y DAG paginado, archivos/árbol de commit, padres y fuentes históricas; pares HEAD/índice/disco/buffer/revisión/ausencia explícitos | Fixtures con merges, raíz, unborn, shallow, cambios externos y entregas obsoletas; Git y fuentes intactos |
| G2, interfaz de consulta | Distribuciones persistentes, divisores, pestañas, diff paralelo/unificado, menús reales y navegación TUI | Flujos equivalentes GUI/TUI, foco, ratón, barras, resize/DPI sintético y lectura sin I/O al pintar |
| G3, herramientas de consulta | Historia por archivo, autor por línea, búsqueda, parches y apertura lateral histórica | Codificaciones/EOL/bytes, historial con rutas cambiadas, límites de trabajo y cancelación |
| G4, escrituras locales M5 | Índice, stage parcial, commit/amend, ramas/etiquetas y restore | Locks, preflight y fallos en publicación; buffers sucios protegidos y recuperación verificable |
| G5, integración M5 | Merge, rebase, cherry-pick, revert, reset y stash; resolución de tres vías | Conflictos, continuar/cancelar, reinicios y modificaciones externas sin pérdida silenciosa |
| G6, red y ampliaciones M5 | Clone/fetch/push/pull, credenciales/upstream; después worktrees, submódulos, LFS/firma según cobertura | Transporte embebido real sin git.exe/auxiliares implícitos; progreso, cancelación y fixtures controlados |

M2 conserva las puertas de solo lectura. La petición de mayoría de herramientas
amplía el alcance planificado de M5; no autoriza ejecutar acciones destructivas
sobre un repositorio personal durante esta investigación.

No basta encontrar una API gix con nombre parecido. Antes de cada bloque se fija
qué cubre realmente la dependencia y qué construye CIDE en Rust, con pruebas de
comportamiento. Escrituras de varios archivos no son atómicas por publicar el índice
atómicamente: deben tener diario recuperable y resultados parciales explícitos.
El undo del editor no puede revertir operaciones de repositorio por su cuenta.

## Evidencias y continuidad

Archivos de trabajo: `work/rea-sublime-merge-{default-merge,default,theme,pe}-evidence.json`,
catálogo derivado `work/rea-sublime-merge-command-catalog.json` y primer intento
nativo `work/rea-sublime-merge-native-attempt.json`. Los bundles REA conservan sus
IDs y cobertura; no se atribuye autoridad REA al parser posterior de recursos.
El resumen portable se registra en `docs/evidence/sublime-merge-rea-study.json`.

Este bloque no cambia el editor ni requiere repetir sus suites cerradas. Se validan
la integridad de evidencias y enlaces y, por separado, la configuración/decompilación
del motor. El siguiente bloque de construcción es G1; no se vuelve a dibujar un
panel decorativo antes de contar con las fuentes y consultas que lo sostienen.

## Anotaciones por línea — 2026-11-06

[Contrato y evidencia](G3_BLAME.md). GUI/TUI usan `jc_git::blame` sobre un commit
fijado, primer padre y ruta exacta, con el único motor de diff existente.
Página de 128 líneas, recorrido de 128 commits, límites explícitos y filas sin
autor inventado al alcanzar frontera shallow/cuotas. El snapshot confirmado
permanece separado del buffer sucio. Un solo worker y aceptación en poll.
GUI: Anotaciones/L, N/P páginas, Intro/clic en atribución abre el commit responsable,
Volver a anotaciones/Esc recupera viewport/foco. TUI: L, N/P, E, Intro, Esc/B y H.
No se siguen renombrados/copias/todos los padres, ni se añaden escrituras Git.

Once casos nuevos dirigidos aprobaron por suites:5 core,2 entrega,2 GUI,2 TUI;
un cierre adicional revalidó solo el caso de cuota afectado. Fmt, Clippy selectivo
all-targets y release workspace offline aprobaron. Sin suites completas nuevas,
HWND, ConPTY, escritorio, CI o push. Sin dependencias ni cambio al lock.
Binarios actuales: `target/blame-next/release`, anteriores preservados.
Los límites físicos/globales de M2 siguen abiertos; IME excluido y Narrador aplazado.
Juan está trabajando: no tomar escritorio ni reiniciar su instancia para revisar.

## Historial por archivo y Esquema ocultable — 2026-11-06

[Contrato y evidencia](G3_PATH_HISTORY_OUTLINE.md). GUI/TUI consultan el historial
de una ruta exacta mediante el núcleo Git independiente: primer padre, inicio
fijado, 24 coincidencias/128 commits por página, cursor ligado y fronteras shallow
explícitas. Listado sin blobs/worktree; comparación mediante el modelo existente.
Un solo worker/controlador, aceptación únicamente en poll y una página retenida.
GUI: Historial del archivo/A, Más antiguos/N, revisión para comparar y regreso.
TUI: A, Intro, N, R/F5 y Esc/B/H. Archivo limpio activo admitido dentro del proyecto.

Esquema GUI: × para ocultar; botón Esquema inferior para recuperar. Cancela tareas
propias y conserva bytes/undo; doble clic/Intro navega y Ctrl abre lateralmente.
No hay nuevos proveedores semánticos ni escrituras/red Git. Trece casos nuevos
dirigidos aprobaron por suites (5 core,2 entrega,2 GUI Git,2 TUI,2 Esquema).
Formato, Clippy selectivo all-targets y release workspace offline aprobaron.
Sin nueva corrida completa de tests, HWND, ConPTY o escritorio: Juan sigue trabajando.
Binarios actuales: `target/path-history-next/release`, anteriores preservados.
No hay dependencias/lock/CI/push nuevos. IME excluido, Narrador aplazado y aceptación
física/global M2 sin ampliar. Próximas herramientas G3: anotaciones y búsqueda,
separadas de la revisión visual del bloque actual.

## Guardar todo corregido — 2026-11-06

[Publicación nueva en Windows](SAVE_ALL_NEW_PUBLICATION.md). Se reprodujo el
rechazo WinError5 de enlaces duros; create-only ahora publica el temporal cerrado
con MoveFileExW sin reemplazo ni copia entre volúmenes. Destino concurrente,
bytes originales, undo y rutas largas conservados; ReplaceFileW existente intacto.
Tres regresiones nuevas de archivos y un test del parser aprobaron. Los tres
flujos dirigidos de Guardar todo aprobaron en HWND oculto, más ocho controles
comunes del fixture. No se repitieron Git, suites completas ni escritorio.
Fmt, Clippy selectivo all-targets y release offline aprobaron. Binarios actuales:
`target/save-all-next/release`, rutas anteriores preservadas. El fallo puntual
anterior está corregido; no se infiere una aprobación de toda la suite HWND/M2.
Evidencia: `docs/evidence/save-all-new-publication-2026-11-06.json`.

## Rediseño visual Git — 2026-11-06

[Diseño implementado y límites](G2_GIT_VISUAL_REDESIGN.md). Barra de 42 dp,
controles planos, ubicaciones adaptables, historial con cambios locales, archivos
en dos líneas y diff amplio con tema oscuro. Corregidos los hits ocultos de
pestañas que interceptaban la barra Git y la identidad del resumen histórico.
Sin nuevas operaciones Git/TUI/dependencias ni cambios de recuperación.

111 casos GUI/entrada aprobaron y el cierre dirigido posterior aprobó 31;
112 casos distintos en total, no una nueva corrida completa del workspace.
Formato, Clippy workspace/todos los destinos offline y release aprobados.
25 comprobaciones HWND dirigidas Git aprobaron con PATH vacío, fixture e índice/
objetos/configuración intactos. La suite HWND completa sigue detenida antes de
Git en Guardar todo por acceso denegado; también se reproduce con el binario previo.
No se declara aprobada esa suite. Evidencia y fallos conservados en el JSON del bloque.

Binarios actuales: `target/git-redesign-next/release`, anteriores preservados.
[Imagen real del pintor GDI](evidence/g2-git-redesign-native.png), con datos de prueba;
no es una captura física. Se observaron apertura visible, maximización y comparación
por clics; la última revisión fue interrumpida por Esc. No se confirmó salida de la
última instancia de prueba. Juan está trabajando: no tomar escritorio para cerrar
ese pendiente. Recuperación personal deshabilitada en las pruebas.
Mostrar el resultado antes de agregar funciones. IME excluido, Narrador aplazado,
segundo monitor/latencia global sin verificar; CI y push no ejecutados.

# M2 — Matriz de cierre

## Distribución Git y comparación paralela — 2026-11-06

[Alcance y evidencia](G2_GIT_LAYOUT_COMPARISON.md). La GUI ofrece una, dos
(archivos a izquierda/derecha) o tres columnas, divisores arrastrables, adaptación
compacta y preferencias recuperables v5 con lectura v1–v4. Un proyector acotado
sirve comparación unificada/paralela de buffers, revisiones y pares locales;
alinea filas, muestra huecos, barras y recorte Unicode sin editar fuentes.

107 casos GUI/entrada aprobaron (101+6), quince nuevos; cierre dirigido de 26
tras ajustes de cuotas y anchura visible. No es una nueva corrida runtime del
workspace. Formato, Clippy todos los destinos y release offline aprobados.
147 comprobaciones HWND verifican las cuatro distribuciones y la captura nativa
de barras/divisores con PATH vacío y fuentes/metadatos Git intactos. El harness
se corrigió para pintar el HWND oculto y estabilizar la primera vista del fixture
largo antes del snapshot de invariantes; se conservan los intentos iniciales.
Binarios: `target/g2-layout-next/release`; anteriores preservados.

Este es el bloque de layout y comparación, no todo G2/G3. Siguen árbol/pestañas
Git, historial por archivo, anotaciones y exportación de parches. Escrituras/red
mantienen M5. No cambia la TUI ni el núcleo Git, no se añaden dependencias/CI.
La revisión física no se repitió: queda el pendiente anterior de escritorio,
con IME excluido, Narrador aplazado y traslado entre monitores sin verificar.

## Comparaciones locales Git y núcleo independiente — 2026-11-06

[Contrato y evidencia](G1_LOCAL_SOURCE_PAIRS.md). GUI/TUI distinguen HEAD → buffer,
índice → buffer, preparados HEAD → índice y cambios en disco índice → disco.
Presencia/ausencia y modo son visibles incluso sin cambios de contenido. jc-git
comparte el lector std-only jc-fs con jc-files, sin depender del editor; el
consumidor público Rust/gix funciona sin host ni git.exe. Un solo controlador
versionado sirve consultas históricas/locales; pintar no lee fuentes ni acepta
entregas. Los buffers sucios, su revisión y los metadatos Git quedan intactos.

744 nombres distintos aprobaron por suites/cierre dirigido, ocho nuevos; no hubo
una corrida integral final única. Se conservan los intentos fallidos de fixtures
y del mensaje TUI, corregidos y revalidados en las suites afectadas. Pasaron
formato, Clippy todos los destinos, release offline y 144 comprobaciones HWND,
con PATH vacío. Binarios actuales: `target/m2-git-pairs-next/release`; anteriores
preservados. No hay nueva afirmación ConPTY, pantalla física o rendimiento global.
La captura de escritorio siguió negra; no se envió entrada y la instancia previa
sigue pendiente de salida normal. IME excluido, Narrador aplazado y segundo
monitor sin verificar. CI dormido; escrituras/red Git permanecen en M5.

Siguiente cierre M2: revisión interactiva final del espacio Git y salida normal
cuando el escritorio permita entrada/captura. La expansión G2/G3 y reutilización
en OmniCore/OmniCoder conservan su plan separado; no se declaran ya integradas.

## Consulta histórica básica y núcleo reutilizable — 2026-11-06

[Contrato y evidencia](G1_HISTORICAL_READS.md). Commit → archivos → comparación
con primer padre está conectado en GUI/TUI, sin sustituir el buffer. jc-git ofrece
lecturas históricas y el único modelo de comparación, con consumidor externo probado.
Binarios vigentes: `target/m2-history-next/release`; anteriores preservados.
734 pruebas integrales iniciales y cierre dirigido cubren 736 casos distintos;
tras el traslado del modelo se revalidaron core/adaptador. Pasaron Clippy/release,
142 checks HWND y siete ConPTY. No hay corrida integral final de 736 ni aceptación
física: escritorio denegó entrada y captura negra, con instancia propia pendiente
de salida normal. IME excluido, Narrador aplazado y CI dormido.
Siguiente bloque de M2: pares explícitos HEAD/índice/disco/buffer. No se habilitan
escrituras ni se amplía el cierre a todo G1–G3.

## Selector con tema y espacio Git nativo — 2026-11-05

[Implementación y límites](M2_GIT_WORKSPACE_NATIVE.md). Historial local, archivos y
comparación tienen áreas propias, con tema oscuro y datos reales del motor Rust.
Abrir carpeta/archivo usa el selector de CIDE. Binarios vigentes:
`target/m2-git-ui-next/release`; se preservan todas las rutas previas.
718 casos aprobaron por suites/cierre dirigido, 139 checks nativos y revisión
interactiva del selector, Git, comparación, detalles y salida normal. No fue una
sola corrida integral. Historial de primer padre hasta 24 commits; grafo completo,
diff de revisiones históricas y escrituras Git no se declaran implementados.
IME excluido, Narrador aplazado y traslado entre monitores sin verificar.

## Prioridad vigente de M2

Juan excluyó IME real del cierre solicitado: no le interesa instalar fuentes o
idiomas. No es un requisito pendiente que bloquee este cierre. Narrador continúa
aplazado. Se integra [selección histórica](M2_SELECTION_HISTORY.md), compartida
entre GUI/TUI y recuperable al reiniciar, sin segundo historial de contenido.
La consulta WMI local informó una pantalla física activa; el traslado entre
monitores no puede afirmarse con este equipo. Evidencia y estado final en VALIDACION.

708 pruebas integrales, 139 checks nativos GUI y nueve ConPTY aprobaron; también
los reinicios normales/periódicos y la prueba de selección con el worker real.
Ctrl+Inicio/Fin y selección hasta extremos están conectados en ambas interfaces;
la inserción limpia anclas degeneradas. Binarios: `target/m2-selection-next/release`.
La prueba interactiva inicial de selección Undo/Redo y salida normal pasó; la
ventana final se pudo observar, pero Windows volvió a denegar entrada de teclado.
No confundir ese bloqueo con un cierre aprobado de todas las pruebas físicas.

[Navegación lateral](M2_SIDEBAR_NAVIGATION.md) sustituye la rueda que movía la
selección TUI y conecta la que faltaba en Archivos GUI. El viewport es propio
de la vista del árbol, con barras navegables, hits por origen/generación/revisión
y revelado explícito por teclado. Símbolos no consume Ctrl+RePág/AvPág.
Pasaron 254 pruebas selectivas, 138 checks HWND y siete ConPTY. Binarios vigentes:
`target/m2-sidebar-next/release`; el bloque Git previo está en `223b050`.
El cierre integral y los límites se registran en VALIDACION; los apartados que
siguen conservan la historia de versiones anteriores, no rutas recomendadas.

La regresión integral final con Rust 1.88.0 aprobó 683 pruebas, cero fallos;
tres helpers se ejecutan desde sus padres. Formato, Clippy workspace/todos los
destinos y release offline aprobaron. Es cierre automatizado local: las puertas
físicas de la tabla siguiente permanecen independientes.

| Pendiente actual de aceptación | Tratamiento |
|---|---|
| IME de composición instalado | Excluido por Juan del cierre solicitado. Se conserva el soporte existente; no se instalan fuentes ni idiomas. |
| Traslado entre monitores | DPI 96/144/192 probado sintéticamente; falta segundo monitor activo para la aceptación física. |
| Pantalla y terminal físicas | HWND/ConPTY y tiempos de confirmación de pintura no equivalen a latencia del compositor/panel ni a todos los terminales. |
| Narrador/accesibilidad avanzada | Diferidos expresamente por Juan; no reabrir como prioridad. |
| Selección exacta anterior a Undo | Integrada con metadatos acotados por transición y CIDEAPP2 compatible con lectura APP1; evidencia en el bloque de selección histórica. |
| Archivos editables de varios GB | Plan independiente; evidencia aislada paginada no implica integración al editor. No usar este cierre para anunciarla. |

Las operaciones Git de escritura/red y proveedores AVM/semánticos no son
pendientes que deban añadirse a M2 para suplir estas puertas de aceptación.

[Motor Git embebido](GIT_EMBEDDED_ENGINE.md): estado y blobs HEAD/índice ya
usan gix 0.90.0 dentro de GUI/TUI, sin instalar ni buscar Git. Sesiones v4
con lectura de las anteriores y licencias conservadas. 533 pruebas por suites,
135 checks nativos GUI con PATH vacío, formato, Clippy todos los destinos y
release workspace aprobaron. Binarios en `target/embedded-git-next/release`.
El proveedor externo anterior queda sustituido; sus informes siguientes son
históricos. Cuotas y cancelación cooperativa no cierran RAM/plazos duros
globales. Submódulos y formatos especiales requieren otro bloque; escrituras
y red siguen en M5. Narrador/accesibilidad avanzada permanecen aplazados.

[Git integrado con el flujo de OmniCore/OmniCoder](M2_GIT_INTEGRADO_OMNI.md):
detección automática del Git instalado, sin selector GUI, conservando runner
Rust y lectura acotada. Pasaron 135 checks nativos integrales con Git real,
75+6 pruebas GUI y prueba del resolver; Clippy y release GUI/TUI aprobaron.
Ejecutables actuales en `target/git-integrated-next/release`.

La [corrección de estado Git GUI](M2_GIT_GUI_STATUS_UX.md) reemplaza el indicador
decorativo por configuración/consulta accesible, conserva la elección de Git,
actualiza después de guardar y al volver a CIDE, y corrige el uso de la señal
de entrega pendiente como si fuera estado aceptado. La leyenda de F6 se retira
solo en GUI. Pasaron 75+6 pruebas GUI y 134 comprobaciones nativas integrales,
con fuente e índice sintéticos intactos. Ejecutable: `target/git-ux-next/release`.

El [bloque de marco, pestañas y paneles](M2_SHELL_PANELS_MENUS.md) coloca las
dos flechas a la izquierda, Nuevo/lista de abiertos a la derecha y añade
delimitadores. Código tiene panel derecho independiente con símbolos y consultas
léxicas explícitas; Archivos permanece a la izquierda. La barra inferior permite
ocultar ambos y resume lenguaje/Git sin activar proveedores durante el dibujo.
Los [menús auditados](M2_MENU_AUDIT.md) pasan de 109 a 73 entradas compartidas,
sin duplicados ni títulos vacíos de Compilar/Ayuda en TUI.

El escritorio reprodujo y corrigió el fallo de redimensionado: la limpieza de
captura del editor cancelaba la captura nativa al recibir WM_SIZE. Se registra
la propiedad de la captura y se liberan solo las adquiridas por el editor.
El arrastre de esquina cambió realmente 800×560 a 700×500; maximizar mostró
ambos laterales correctamente. La misma inspección detectó que los símbolos
ignoraban la apertura lateral: el controlador compartido ahora usa una segunda
vista del mismo documento, conservando cursor de origen y deshacer único.

La validación GUI observó 73 nombres de pruebas aprobados entre corrida amplia
y cierre dirigido; seis pruebas de entrada aprobaron en la corrida anterior.
TUI pasó sus 152 pruebas en ejecución serial con acceso normal a sus fixtures.
La ejecución restringida anterior falló en procesos Git/enlaces y se interrumpió
con 0xc0000409; no se presenta como aprobación global. La regresión de menú
compartido y dos de integración de interacción aprobaron. Pasaron 16 checks
nativos de marco y 16 de interacción, incluida captura nativa conservada.
Clippy de todos los destinos de las tres capas pasó. Informes en
`work/m2-panels-*.log`; capturas nativas en
`work/native-gui-smoke/20261105T180402Z-8981a99fa4f4` y
`work/native-gui-smoke/20261105T181020Z-7b2a34ac9edd`.

Ejecutables de este bloque: `target/m2-panels-next/release`, con los anteriores
conservados. La recuperación gráfica v2 guarda visibilidad de Código y admite
metadatos v1. Las pruebas usan depósitos y documentos sintéticos.

El cierre final release GUI/TUI y formato aprobaron. En la instancia aislada
`work/m2-panels-visual-dr4umpp4` se confirmó Ctrl+Intro sobre un procedimiento:
dos grupos visibles del mismo documento, con cursor de origen conservado.
Los botones inferiores ocultaron Código y Archivos de forma independiente,
y el desplegable de pestañas abrió la lista filtrable de los tres documentos.
Se salió normalmente. Las 23 pruebas TUI seleccionadas por fuente pasaron tras
el ajuste compartido de apertura lateral, sin repetir toda la suite.

El bloque de avisos y borradores integra una confirmación modal propia
con colores y tipografía compartidos con CIDE. Cancelar recibe el foco
inicial y el descarte conserva la validación del documento y su revisión.
La numeración de «Sin título» elige el primer número libre del catálogo
abierto. Una sesión vacía vuelve a 1, incluso si el contador histórico
persistido era mayor. Se conservan los nombres y los cambios de borradores
recuperados, así como la compatibilidad del formato de sesión.

La revisión inicial se interrumpió por acceso denegado y capturas negras.
Al recuperar el escritorio se observó que el aviso era demasiado grande,
sin separación suficiente del editor y centrado en pantalla, no en su dueño.
El ajuste usa 420 × 176 píxeles de diseño, escala por DPI, centra sobre CIDE
y limita la posición al área útil del monitor. El cuerpo usa el tono elevado,
el encabezado un tono más claro y el borde otro contraste del tema.
Escape conservó la edición sintética y la ventana de prueba salió normalmente
después de deshacerla. La ventana anterior ya no estaba abierta.

El cierre de este ajuste pasó 65 pruebas de biblioteca GUI y seis de entrada,
incluida la posición con coordenadas negativas y límites de monitor a
96/144/192 DPI, y Clippy de todos los destinos GUI. La composición real con IME,
el traslado entre monitores y la latencia física siguen abiertos; Narrador
y accesibilidad avanzada continúan diferidos por Juan.

La revisión de escritorio del ejecutable `target/m2-dialog-compact/release`
confirmó el aviso compacto centrado, encabezado y cuerpo elevados y borde claro.
Intro canceló conservando el carácter sintético. Un Deshacer, cierre de la última
pestaña y Nuevo volvieron a «Sin título 1.txt». La instancia salió mediante
Alt+F4 sin terminación forzada. Fixture aislado `work/m2-dialog-visual-0z1x3exa`,
recuperación desactivada, sin documentos personales. Fmt, Clippy y release
aprobaron. La apariencia ya no queda bloqueada por el fallo anterior de captura.

Pasaron 63 pruebas de biblioteca GUI y seis de entrada en el cierre
amplio. Después pasaron las regresiones dirigidas del botón por defecto,
tipografía y estilo de controles al cambiar foco, y sesión vacía con
contador histórico 5. Clippy y release aprobaron. El binario de
`target/m2-dialog-final/release` pasó el recorrido nativo integral y
tres procesos de recuperación. Informes en
`work/native-gui-smoke/20261105T060642Z-c610acd8dd1c` y
`work/recovery-gui/20261105T060645Z-1459102e/report.json`.

Juan dejó Narrador y la accesibilidad avanzada como pendientes para
concentrar M2 en interfaz, desplazamiento y edición. El diagnóstico
histórico se conserva abajo. IME real y cambio entre monitores quedan
pendientes por las características del equipo disponible.

El bloque de pulido usa `target/m2-polish-next/release/cide-gui.exe`.
La pestaña activa vuelve a quedar visible al estrechar la ventana o
cambiar el catálogo. El desplazamiento manual de pestañas se conserva
mientras la geometría permanece estable. Las franjas demasiado pequeñas
se omiten. Barras, márgenes y minimapa se ajustan al DPI de la ventana.

Las fracciones de rueda y los movimientos de barra que conservan el mismo
origen ya no reconstruyen el frame. El recorrido integral nativo aprobó
los flujos de selección, edición, undo, guardado, búsqueda, paneles y
pestañas. La primera ejecución restringida falló en la preparación de
fixtures de guardado. El mismo ejecutable aprobó el recorrido completo
con acceso normal a sus archivos sintéticos.

Se añadieron regresiones de pestaña activa al reducir ancho, geometría de
barras a 96/144/192 DPI y reutilización del frame con media muesca de rueda.
Pasaron 57 pruebas de biblioteca GUI y seis de entrada, Clippy de todos
los destinos GUI y compilación release offline.

El cierre siguiente corrigió AvPág/RePág para usar las filas visibles
del grupo activo. La regresión dirigida pasó en ambos grupos, con y
sin panel inferior, selección mediante Mayús y bytes conservados.
Clippy, release y las 15 comprobaciones nativas de interacción volvieron
a pasar con ese cambio. Los informes nativos están en
`work/native-gui-smoke/20261105T050507Z-1fff631fb47c` (integral)
y `work/native-gui-smoke/20261105T050850Z-99c8e20148a3` (interacción).

`scripts/measure-m2-gui-scroll.py` ejecutó 360 pasos sintéticos de rueda
a una frecuencia solicitada de 60 Hz, con ventana visible y un documento
de 4 400 000 bytes y 100 000 líneas. En 5987,35 ms, la confirmación
de entrada y pintura dio mediana de 3,35 ms, p95 de 4,79 ms y máximo de
6,40 ms. Ninguna muestra superó 16,67 ms. El retraso de programación
del generador tuvo p95 de 0,58 ms. La fuente de prueba quedó intacta.
El informe y las 360 muestras están en
`work/m2-visible-scroll-sls88g85/report.json`.

El punto de medida es el retorno de WM_PAINT después de procesar la
rueda en el HWND propio. La prueba no incluye la presentación física del
compositor ni mide interacción con otras aplicaciones. Se conserva la
puerta de latencia física de M2.

El ajuste posterior del borde de pestañas reemplazó el relleno compuesto
por un único contorno con curvas superiores. Se inspeccionó el bitmap
`work/native-gui-smoke/20261105T053008Z-1635151be0ef/native-shell.bmp`.
Pasaron las 16 comprobaciones del marco, Clippy y release. Binario de
este ajuste en `target/m2-tabs-next/release/cide-gui.exe`.

Ante el aviso de pérdida de cambios informado por Juan, se repitieron
las tres regresiones de recuperación GUI y un recorrido real de cierre
y reapertura con tres procesos aislados. Se restauraron cambios sin
guardar, cursor y deshacer, y el original permaneció intacto. Informe
en `work/recovery-gui/20261105T053010Z-c919eda6/report.json`.
El código de cerrar una pestaña conserva una confirmación de descarte,
mientras cerrar la ventana confirma la sesión cuando la recuperación
está habilitada. Se solicitó identificar cuál cierre produjo el aviso
reportado; el recorrido del usuario sigue sin aislarse.

Juan precisó que el aviso apareció al cerrar la ventana completa y después
indicó que no pudo reproducirlo. La revisión encontró una ruta de descarte
global cuando `checkpoint_session` devolvía recuperación desactivada.
Se retiró esa confirmación global. La salida prepara primero la copia de
sesión y conserva la ventana ante errores o cambios pendientes con la
recuperación desactivada. No se atribuye el aviso original a una causa
reproducida.

La protección de salida pasó cuatro regresiones de recuperación GUI,
Clippy y release. El binario final de `target/m2-exit-next/release`
pasó tres procesos de cierre/reapertura conservando cambios, cursor y
undo, además de 16 comprobaciones del marco. Informes en
`work/recovery-gui/20261105T055007Z-9ec566ea/report.json` y
`work/native-gui-smoke/20261105T055008Z-d49fb4b8c96b/native-report.json`.

La aceptación distingue los resultados automatizados en Windows de la
interacción en un escritorio. La TUI y la GUI comparten el documento, las
transacciones, los grupos y las tareas. La validación de OpenEdge y los
proveedores de lenguaje avanzados pertenecen a M3–M4.

| Puerta | Evidencia y estado |
|---|---|
| Regresión del workspace | 654 pruebas aprobadas en la ejecución serial final. Los tres auxiliares marcados como ignorados se invocan desde sus pruebas padre. Pasaron la creación del enlace de prueba y la integración Git que fallaron en el entorno restringido. |
| TUI en terminal privada | 23 comprobaciones entre explorador, ratón, español/Markdown y recuperación periódica. Se usa ConPTY propio y archivos sintéticos. |
| GUI en Windows | 131 comprobaciones integrales con ventana propia, entre ellas cambios de DPI simulados a 96/144/192. Dos recorridos adicionales de 16 comprobaciones cada uno cubren herramientas de código y marco. Recuperación periódica confirmada en siete comprobaciones con tres procesos aislados. |
| Interoperabilidad | CIDE y Notepad++ intercambiaron texto Unicode en ambos sentidos en el escritorio. Un solo Deshacer restauró el documento de CIDE. La sesión solo expuso un monitor y no presentó un IME de composición identificable. |
| Accesibilidad | UI Automation verificó rangos, foco, cursor y selección. El Resumen de voz de Narrador confirmó el nombre del archivo y la lectura de una línea. La lectura de la selección con Narrador sigue abierta. |
| Rendimiento | GUI visible hasta el bucle de mensajes en espera. Para 4095 bytes, mediana de 39,39 ms, p95 de 46,36 ms y máximo de 5,09 MiB privados. Para 4 MiB, 56,64 ms, 60,57 ms y 10,05 MiB privados. La rueda avanzó y retrocedió correctamente en un documento de 100 000 líneas. Quedan por medir los intervalos entre cuadros y la latencia física. |
| Archivos de varios GB | La prueba aislada de lectura paginada sigue vigente. La instantánea privada y el documento editable de varios GB conservan su plan separado. |

## Resultados de escritorio del 4 de noviembre de 2026

Windows 10.0.26300, GUI de `target/uia-next/release`, recuperación
desactivada y archivos sintéticos. Se observó el documento abierto y su
selección de 4463 unidades UTF-16 mediante un cliente de UI Automation con
la ventana visible. La prueba posterior con Narrador se registra abajo.

Windows enumeró un monitor de 1536 × 864 en coordenadas de escritorio.
Los perfiles de teclado cargados fueron español de México e inglés de
Estados Unidos. Ninguno devolvió descripción o archivo de IME por IMM.
La composición real y el traslado entre monitores de distinta escala
siguen abiertos. La evidencia simulada de 96/144/192 DPI se conserva.

El diagnóstico posterior confirmó que el avance de la secuencia y el cambio
de propietario no bastan para demostrar que se publicó texto. En la
ejecución restringida, `SetClipboardData` devolvió acceso denegado (error 5)
y Pegar quedó deshabilitado en Notepad++. Al repetir con el mismo binario
fuera de esa ejecución, CIDE transfirió dos líneas con acentos, eñe y emoji.
Notepad++ devolvió las tres líneas resultantes, incluidos los caracteres
japoneses. CIDE recibió el texto y un solo Ctrl+Z restauró las dos líneas
originales. Se usaron instancias y documentos sintéticos, sin modificar el
transporte de producción.

En la misma ventana visible se comprobó el ajuste de una pestaña con nombre
largo junto a otra normal. El fondo redondeado se dibuja una vez dentro de
los límites de la pestaña, y el título y el cierre tienen áreas separadas.
El título usa puntos suspensivos según el espacio real de la fuente.

La medición de arranque visible tomó siete muestras después de una de
calentamiento. El punto de medición exige un marco visible del proceso
propio y el retorno de `WaitForInputIdle`. Es una medición de arranque
y memoria privada en ese punto. El informe local con las muestras está
en `work/m2-gui-startup-d393e881744e4636bf3531e8d3c25810/report.json`.
Se reproduce con
`scripts/measure-m2-gui-startup.py --exe target/uia-next/release/cide-gui.exe --visible`.
El tiempo hasta el primer cuadro físico y el desplazamiento sostenido
quedan fuera de esa medición.

Se repitió la medición visible con `target/desktop-next/release/cide-gui.exe`
y `--lines 322639`, un archivo de 4 194 307 bytes. En siete muestras
medidas la mediana fue de 56,64 ms, el p95 de 60,57 ms y el máximo de
10,05 MiB privados. Informe local en
`work/m2-gui-startup-e25aa5daea704d5ba6a25d0fc4220207/report.json`.
El mismo binario pasó 16 comprobaciones del marco y 13 de UI Automation.
También pasaron las tres pruebas dirigidas de pestañas.

## Narrador, desplazamiento y tipografía de menú

En una ventana visible con un documento sintético de 100 000 líneas,
el Resumen de voz de Narrador mostró `CIDE-M2-NARRADOR.txt` y «Prueba
de lectura: la brújula azul.». Se verificó la transcripción de Narrador.
La salida por altavoces no se midió. Al pedirle leer la selección,
Narrador registró «Nothing Selected» aunque un cliente independiente de
UI Automation leyó el texto seleccionado. Se corrigió el proveedor para
anunciar selección simple en lugar de múltiple, de acuerdo con su soporte
lineal. La discrepancia persistió en la repetición.

Con Narrador desactivado, tres ráfagas de rueda movieron la primera fila
visible de 1 a 91, luego a 391 y de regreso a 1. Cada captura mostró
el texto de la fila correspondiente y el cursor se conservó en la primera
línea. Los recorridos de automatización y captura duraron 242, 195 y 264 ms.
Estos tiempos incluyen la herramienta de escritorio y quedan fuera de
una medición de cuadros o de latencia física.

La barra y los desplegables comparten Segoe UI de 14 píxeles a 96 DPI.
El menú ajusta altura y márgenes con la escala, con espacio separado para
la etiqueta y el atajo. Se inspeccionó Archivo en escritorio y se
repitieron 16 comprobaciones del marco y 13 de UI Automation con
`target/m2-font-next/release`.

### Diagnóstico posterior de selección y arrastre

La compilación temporal registró consultas de UI Automation sin contenido
de fuente. Narrador recibió una selección no vacía de bytes 0–36 con foco
activo. Su Resumen de voz mostró «Prueba de lectura: la brújula azul.
selected». Esto confirma el anuncio de selección al modificarla. El comando
explícito Narrador+Mayús+Abajo siguió produciendo «Nothing Selected»,
incluso en una instancia abierta fuera de la ejecución restringida.
Cambiar a modo de exploración produjo una selección virtual distinta
(«new line»). El diagnóstico separa la selección del editor del cursor
virtual del lector. La causa del comando explícito sigue abierta.
El control con Notepad++ no produjo una lectura explícita concluyente.

La prueba de arrastre encontró un fallo independiente. La liberación
automática de captura contemplaba selección lineal y rectangular, pero
omitía el arrastre de barras. Al capturar el ratón, CIDE lo liberaba
inmediatamente y cancelaba el arrastre. Se corrigió la condición y se
añadió una comprobación de captura, recorrido y cursor en el smoke de
interacción. En escritorio, minimapa y barra vertical recorrieron desde
la primera línea hasta la 96 313 y de regreso conservando el cursor.

La instrumentación temporal midió nueve composiciones en tres ráfagas
con al menos tres muestras consecutivas. Preparación y envío GDI dieron
mediana de 2,635 ms y p95 de 3,601 ms. Los seis intervalos entre entradas
de pintura dentro de esas ráfagas dieron mediana de 10,042 ms y máximo
de 17,075 ms. No hubo errores de dibujo. El informe local está en
`work/m2-scroll-8hhpijof/metrics.json` y las marcas en `probe.log`
de esa carpeta. Es una muestra corta de arrastre y excluye la espera
hasta presentación del compositor. Quedan pendientes una carga sostenida
de mayor duración y la latencia física. La instrumentación se retiró
del código de producción después de la captura.

La sesión sigue mostrando un solo monitor y los perfiles de teclado
español de México e inglés de Estados Unidos, sin descripción ni archivo
de IME de composición por IMM. Se mantienen abiertos el cambio real
entre monitores y la composición/confirmación/cancelación con IME.

La repetición final de Narrador no fue válida. El proceso de Narrador
dejó de aparecer antes de ejecutar el comando. El lanzamiento directo
en la sesión normal devolvió WinError 740 (requiere elevación). Se pidió
abrir Narrador manualmente para repetir con el lector activo. No se
atribuyó el resultado de esa repetición al proveedor ni se implementaron
atributos de texto sin evidencia de que Narrador los solicite. El binario
sin instrumentación pasó 15 comprobaciones de interacción (incluida la
nueva regresión de captura) y 13 de UI Automation. Clippy del paquete GUI
con todos sus destinos pasó sin advertencias.

Juan abrió Narrador manualmente y se repitió el recorrido con la compilación
sin instrumentación, en una instancia de escritorio con recuperación
desactivada. Se confirmó que Narrador permaneció activo durante el comando
y que UI Automation conservó «Prueba de lectura: la brújula azul.» como
selección. El Resumen de voz registró la frase seguida de «selected» y,
al ejecutar Narrador+Mayús+Abajo, «Nothing Selected». Esta repetición es
válida y confirma que el inicio automático fallido no explica por sí
solo la discrepancia. Se mantiene abierta la relectura explícita.
La carpeta sintética utilizada fue `work/m2-scroll-o_cx9859`.

## Recorrido de escritorio pendiente

Usar una carpeta temporal con documentos propios de prueba. Registrar
la versión de Windows y de la terminal, la escala de cada monitor y el
IME empleado.

1. En la TUI, navegar con F6 y aceleradores, abrir al lado, arrastrar
   ambas barras y probar que los atajos que el terminal no entrega se
   puedan ejecutar desde la paleta. Revisar en ancho normal y compacto.
2. En la GUI, probar escala de 100 %, 150 % y 200 %, mover la ventana entre
   monitores y comprobar correspondencia entre dibujo y clic en pestañas,
   Explorador, editor, barras y minimapa.
3. En la GUI, componer texto en un IME instalado. Probar confirmación en
   un paso de deshacer, cancelación, cambio de vista y rechazo de caracteres
   incompatibles con la codificación.
4. Verificar la restauración de un borrador tras reiniciar. Mantener aislado
   el depósito de sesión en las pruebas automatizadas.
5. Registrar lectura de nombres y estados, foco, cursor y selección con
   un lector de pantalla, tanto al editar como al cambiar de grupo.

Los objetivos cuantitativos y el corpus se conservan en el
[plan de M2](PLAN.md). El historial de resultados está en
[validación](VALIDACION.md).

Los registros de la corrida están en `work/m2-close-workspace-final.log`,
`work/m2-close-gui-final.log` y los demás archivos `work/m2-close-*.log`.
La medición de arranque se reproduce con
`scripts/measure-m2-gui-startup.py --exe target/m2-close/release/cide-gui.exe`.
Se mide una respuesta del bucle de mensajes con ventana oculta, con recuperación
deshabilitada y sin garantizar el tiempo hasta el primer cuadro visible.

## Alcance de la implementación GUI

- La ventana actualiza fuentes y geometría al recibir un cambio de DPI.
  Se vuelven a preparar las áreas de interacción y se aplica el marco
  sugerido por Windows. El menú nativo comparte la configuración tipográfica
  de la interfaz. Queda pendiente validar su geometría al cambiar de monitor.
- IME utiliza la composición nativa y coloca la ventana de candidatos cerca
  del cursor. Se aplica el texto confirmado como una edición y se comprueba
  que destino, revisión y codificación sigan vigentes. Las pruebas cubren
  la inserción y el rechazo, sin simular un IME instalado.

[Cambio de DPI en Win32](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged)
· [Procesamiento de composición IME](https://learn.microsoft.com/en-us/windows/win32/intl/processing-the-wm-ime-composition-message).

## UI Automation del editor nativo

El proveedor del marco expone el documento del grupo activo mediante
`TextPattern` y `TextPattern2`. Los rangos capturan identidad de documento,
vista, codificación y revisión. Las peticiones COM llegan al hilo dueño
mediante identificadores opacos y una cola acotada. El proveedor lee el
modelo compartido y publica cambios de foco, selección y texto.

Los clientes pueden leer por rangos, mover extremos por carácter, línea
o documento, consultar el cursor y su geometría, convertir coordenadas en
rangos y aplicar una selección lineal. La lectura admite 64 KiB de fuente
y 65536 unidades UTF-16 por llamada. Los bytes inválidos se representan
mediante escapes en la salida accesible. La selección rectangular, los
otros paneles de la aplicación y las funciones de búsqueda de TextPattern
quedan fuera de este incremento.

`scripts/smoke-uia-gui.py --exe target/uia-next/release/cide-gui.exe`
crea una ventana oculta y un cliente de Windows independiente. Usa un
archivo sintético, recuperación desactivada y ninguna interacción con
otras ventanas. La prueba distingue un evento de foco enviado al marco
de una verificación real con Narrador o NVDA.

[Patrones Text y TextRange de Windows](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementingtextandtextrange).

Con UI Automation integrado, el recorrido de arranque oculto de siete
muestras dio p95 de 36,38 ms y un máximo de 2,39 MiB privados hasta respuesta
del marco. La comprobación del marco y el dibujo fuera de pantalla pasó
16 casos. Estos resultados se registran en `work/m2-uia-startup.log` y
`work/m2-uia-shell.log`.

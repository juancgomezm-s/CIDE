## Memoria y rendimiento del editor completo — 2026-11-06

[Protocolo y límites](M2_EDITOR_PROCESS_PERFORMANCE.md). Procesos reales ocultos:
GUI pico privado 46,00 MiB con ocho documentos de 4 MiB visitados, incluyendo
2,48 MiB del probe; después de cerrar/liberar probe 6,04 MiB. TUI pico17,95 MiB
con fuente16 MiB. 949 composiciones nativas sin errores,160 ciclos editar/deshacer
GUI y24 TUI; fuentes intactas. Corregida retención de píxeles al abandonar Markdown.
Seis pruebas GUI dirigidas, fmt, Clippy GUI all-targets y release workspace offline
aprobados. Cachés reutilizadas, entrega target/m2-process-next/release. Sin escritorio,
clipboard/recuperación personal, dependencias, CI o push. <50 MiB queda acreditado
localmente para estos corpus base/1000×650, no como límite global; siguen pendientes
servicios simultáneos/geometrías extremas y aceptación física. IME excluido/Narrador
aplazado; no se accionaron instancias antiguas.

## Imágenes, sangrías y enlaces Markdown — 2026-11-06

[Contrato y vista nativa](M2_MARKDOWN_MEDIA_LINKS.md). GUI dibuja imágenes
PNG/JPEG/BMP/GIF; las locales cargan en segundo plano y las web requieren clic/Intro.
Listas anidadas conservan niveles y sangría colgante. Enlaces explícitos locales,
al lado, HTTP/HTTPS y anclas con búsqueda cooperativa. TUI usa texto alternativo
y Tab/Intro para destinos, sin píxeles. Ver mantiene check solo para Markdown.
33 casos dirigidos distintos,20 nuevos; fmt y Clippy selectivo aprobados.
HTTP se validó con fixtures privados 127.0.0.1, sin navegador ni Internet.
Entrega target/markdown-media-next/release; rutas anteriores preservadas.
Sin escritorio, portapapeles/recuperación personal, CI o push nuevos.
El bloqueo previo de Computer Use y la aceptación física/global siguen aparte.

## Vista Markdown renderizada — 2026-11-06

[Contrato e imagen nativa](M2_MARKDOWN_RENDERED_VIEW.md). Ver → Vista previa de
Markdown muestra check al activarse y solo admite documentos Markdown. GUI dibuja
jerarquía de encabezados, énfasis real, código, citas, reglas y tablas; TUI comparte
metadata y marca de menú. Solo lectura, bytes/undo intactos y sin dependencias.
13 casos dirigidos distintos aprobados (6 nuevos); Clippy selectivo all-targets
aprobado. Binarios: target/markdown-render-final/release; anteriores preservados.
Computer Use se intentó con instancia aislada: fuente accesible, captura negra,
GetCursorPos acceso denegado al activar y mismo resultado tras selección nueva.
Sin entrada enviada ni aceptación física; se pidió disponibilidad del escritorio.
La instancia propia sigue pendiente de salida normal; no actuar con IDs antiguos.
CI/push no ejecutados; recuperación personal deshabilitada.

## Estado vigente: cierre funcional de menús — 2026-11-06

[Pegado al campo correcto](M2_MENU_FIELD_PASTE.md): GUI y TUI conservan el destino
visible al ejecutar Editar → Pegar; borradores admiten la misma intención que
Ctrl+V y resultados/previews mantienen solo lectura. Ocho regresiones nuevas,
20 casos dirigidos distintos aprobados por suites; formato, Clippy GUI/TUI
all-targets y release workspace offline aprobados. Sin escritorio ni portapapeles
personal. Cachés existentes reutilizadas. Binarios: target/m2-field-paste-next/release.

**M2 no se declara cerrado físicamente.** Lo que resta de aceptación no puede
certificarse bajo la restricción actual de no usar escritorio: revisión visual
final/salida normal de la instancia anterior, latencia física y traslado entre
monitores. No se actuó sobre ventanas o procesos antiguos. RAM global sigue sin
aceptación general; mediciones locales anteriores conservan sus límites.
IME está excluido por Juan y Narrador/accesibilidad avanzada siguen aplazados.
Los bloques G2/G3 adicionales, edición de varios GB y proveedores M3–M5 conservan
sus planes separados; no se inventan funciones para sustituir pruebas físicas.

## Búsqueda de commits y limpieza local — 2026-11-06

[Contrato](G3_COMMIT_SEARCH.md). GUI/TUI usan el núcleo Rust compartido para
buscar mensaje completo, autor/correo o prefijo SHA. Primer padre, inicio fijado,
24 coincidencias/128 commits por página y cursor ligado. Un único worker existente,
aceptación solo en poll y ninguna lectura en prepare. GUI Ctrl+F, TUI F dentro de
Git; Intro ejecuta/abre, F2 campo, Tab foco, N continúa y Esc/B regresa.
Once casos dirigidos aprobaron por suites (5 núcleo,2 entrega,2 GUI,2 TUI).
Fmt, Clippy selectivo all-targets y release workspace offline aprobaron.
[Evidencia del bloque](evidence/g3-commit-search-2026-11-06.json).
Sin escritorio, HWND, ConPTY, portapapeles/recuperación personal, CI o push nuevos.
Binarios actuales: `target/commit-search-next/release`; anteriores preservados.
IME excluido, Narrador aplazado y aceptación física/global M2 sin ampliar.

[Limpieza y caché](LOCAL_BUILD_CACHE_CLEANUP.md): retirados 15,74 GiB lógicos de
intermedios antiguos;340 binarios protegidos verificados sin cambios. Las entregas
se exportan desde target/build-cache con scripts/build-cide-local.py; las pruebas
reutilizan target/embedded-git-next. No repetir suites cerradas por documentación.
G3 sigue limitado a lecturas; operaciones de escritura/red permanecen en M5.

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

# Plan de desarrollo

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

## Prioridad: núcleo Git compartido y alcance de cierre M2

Juan fija un [núcleo Git independiente](GIT_SHARED_CORE.md), integrado en CIDE
y reutilizable por OmniCore/OmniCoder mediante adaptadores. jc-git ya tiene una
base sin dependencias del editor; no hay bindings ni integración de otros hosts
validados todavía. El cierre M2 comprende consulta básica/comparaciones y pulido;
la expansión completa G1–G3 se organiza por separado y no bloquea todo M2.
G4–G6 conservan sus puertas M5. Reutilizar no requiere rediseñar la UI de cada host
ni extraer ahora un repositorio o tres copias del motor.

## Expansión Git y estudio REA de Sublime Merge — 2026-11-05

Juan pidió usar REA sobre Sublime Merge y cubrir la mayoría de sus herramientas.
Se admite el plan [G1–G6 y propuesta GUI/TUI](SUBLIME_MERGE_REA_STUDY.md), sustentado
por [auditoría de capacidades reales](GIT_CAPABILITIES_CIDE_AUDIT.md).
G1 fija consultas históricas y pares de fuentes tipadas; G2/G3 conectan
interfaz y utilidades de la expansión. Solo el subconjunto descrito arriba
pertenece al cierre M2; G4–G6 mantienen escrituras/red en M5.
No se declara paridad ni se habilitan acciones decorativas.

Juan autorizó configurar el motor que faltaba: [Ghidra y JDK para REA](REA_GHIDRA_SETUP.md)
quedan separados de las dependencias y distribución de CIDE. Pasaron diagnóstico, búsqueda real, tres trazas y tres decompilaciones
del binario; las evidencias y sus límites se documentan allí.
Este es un bloque de investigación/configuración: el código funcional del editor conserva la referencia
3600948 y sus ejecutables en `target/m2-git-ui-next/release`. No se repiten sus
suites por cambios de documentación. CI continúa aplazado.

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

## Estado de cierre M2 — 2026-11-04

Actualización de alcance: IME real queda excluido por decisión de Juan; no se
instalan fuentes o idiomas. Narrador continúa aplazado. La [selección histórica](M2_SELECTION_HISTORY.md)
ya tiene implementación compartida y codec de recuperación APP2 compatible con
APP1; 708 pruebas integrales aprobaron. Se conectaron además Ctrl+Inicio/Fin y
sus selecciones y se corrigió la inserción tras ancla degenerada. Traslado entre monitores: no verificado,
Windows informa una pantalla física activa. No ampliar a M3/M5 para suplir
esta limitación de equipo ni confundir la simulación DPI con aceptación física.

Cierre local actual: `target/m2-selection-next/release`, 139 checks GUI y nueve
ConPTY aprobados; recuperación periódica real y lectura de APP1 también.
La interacción inicial con selección Undo/Redo pasó en escritorio y la ventana
salió normalmente. La repetición final de navegación/cierre encontró acceso
denegado al teclado tras poder observar el marco; no es aceptación física global.
El estado y la instancia de prueba pendiente están en VALIDACION.

Actualización 2026-11-05: el Git embebido validado y los ajustes de interfaz
anteriores están guardados en el commit local `223b050`. Se integra el siguiente
bloque [navegación lateral](M2_SIDEBAR_NAVIGATION.md): rueda y barra del
Explorador en GUI/TUI, viewport independiente de selección y atajos globales
desde Símbolos. Ejecutables vigentes: `target/m2-sidebar-next/release`.
254 pruebas selectivas, 138 checks nativos y siete ConPTY aprobaron; la corrida
integral posterior pasó 683 pruebas del workspace con Rust1.88. Puertas actuales
en [matriz de cierre](M2_CIERRE.md).
M2 no se declara aceptado globalmente por pasar pruebas ocultas/privadas:
siguen separadas IME real, traslado entre monitores y latencia física.
Narrador/accesibilidad avanzada permanecen aplazados por Juan.

Requisito autorizado e integrado: [motor Git Rust embebido](GIT_EMBEDDED_ENGINE.md),
sin instalación de Git. `jc-git` enlaza gix 0.90.0 en GUI/TUI, reemplazando
el runner externo para estado y blobs HEAD/índice. Rust 1.88.0 está instalado
solo en el workspace; Cargo.lock y licencias de las transitivas están fijados.
Ejecutables nuevos: `target/embedded-git-next/release`. G0/G1/G2 de lectura
se cierran con la evidencia de VALIDACION.md y las limitaciones allí registradas.
Operaciones de escritura, red, merge/rebase y credenciales siguen en M5;
esta integración no anuncia paridad completa con Git.

[Integración Git automática](M2_GIT_INTEGRADO_OMNI.md), adaptada de los flujos
de OmniCore/OmniCoder: no se pide elegir ejecutable en GUI. Validación nativa
integral de 135 checks y binarios GUI/TUI en `target/git-integrated-next/release`.

Última corrección GUI: [estado Git accesible](M2_GIT_GUI_STATUS_UX.md), con
ejecutable elegido recordado, contador corregido y actualización por eventos.
Leyenda F6 retirada solo de GUI; atajo conservado. Binario GUI vigente en
`target/git-ux-next/release`, con pruebas y límites en la matriz de cierre.

Incremento vigente: [marco/pestañas/paneles](M2_SHELL_PANELS_MENUS.md) y
[menús depurados](M2_MENU_AUDIT.md). Se corrige la captura que impedía
redimensionar por el borde, se separa Código al lado derecho y se añaden
controles inferiores independientes para ambos laterales. Pestañas con
flechas a la izquierda, Nuevo/lista a la derecha y delimitadores. Estado de
validación detallado en la matriz de cierre; ejecutables en
`target/m2-panels-next/release`.

La [matriz de cierre](M2_CIERRE.md) reúne la evidencia y las puertas restantes.
Por indicación de Juan, Narrador y accesibilidad avanzada quedan pendientes.
M2 concentra el trabajo actual en interfaz, desplazamiento y edición.

El bloque de avisos y nombres de borradores sustituye la confirmación
clara de Windows por un diálogo propio con la paleta de CIDE y cancelación
inicial. Los nombres «Sin título» usan el primer número libre del catálogo
vivo, para reiniciar en una sesión vacía y conservar borradores recuperados.
La revisión posterior corrigió el tamaño y centrado: aviso de 420 × 176
píxeles de diseño, cuerpo más claro y borde visible. Pasaron la inspección
de escritorio, cancelación con Intro, reinicio de numeración y salida normal.
Ejecutable vigente: `target/m2-dialog-compact/release/cide-gui.exe`.

El pulido mantiene visible la pestaña activa al reducir ancho, escala las
barras con DPI y evita reconstruir el dibujo cuando la rueda o el arrastre
conservan la misma vista. El recorrido integral nativo pasó. Una medición
visible de 360 pasos de rueda sobre 4,4 MB dio 3,35 ms de mediana y
4,79 ms de p95 hasta confirmación de pintura.

AvPág/RePág ya usan las filas visibles del grupo activo, con selección
y panel inferior, y pasaron su regresión dirigida. Quedan pendientes la
latencia física, IME real y cambio entre monitores de distinta escala.
La validación de composición existente es inyectada. Los ejecutables de
este bloque están en `target/m2-polish-next/release`.

## Estado vigente — recuperación periódica y limpieza segura, 2026-11-03

Recuperación automática en Rust integrada en GUI y TUI: captura cooperativa de
64KiB por turno, cola de un tramo, escritura/verificación en worker y detección de
versiones obsoletas. Programa tras2s sin actividad y fuerza un intento a30s;
un documento pendiente que cambia cancela la captura incompleta. Fallos esperan10s.
Tras la captura inicial, una sesión sin cambios no reescribe sus copias.

La limpieza conserva HEAD/BACKUP verificados, borra únicamente generaciones
obsoletas del slot propio por handles Win32 y rechaza enlaces/archivos desconocidos.
Las escrituras incompletas propias tienen guarda RAII. La limpieza aplazada vuelve
a intentarse sin reconstruir documentos ni avanzar HEAD. El cierre normal espera
el worker y confirma después la última edición. [Contrato](PERIODIC_RECOVERY_RETENTION.md).

20 nuevas pruebas aprobadas por suites/cierres dirigidos, más regresiones de
recuperación y terminación abrupta. Tres procesos reales de GUI y tres de TUI
pasaron7 comprobaciones por interfaz con terminación forzada, fuente original
intacta, dos generaciones protegidas y ausencia de reescrituras en reposo.
Los enlaces duros/junction y Guardar como de fixtures necesitaron ventana local
sin la restricción del sandbox. No se cambiaron garantías para hacerlos pasar.
Fmt, Clippy todos los targets sin advertencias y release offline aprobados;
no corrida runtime global única. Evidencia: `docs/evidence/periodic-recovery-2026-11-03-*`.

Ejecutables actuales: `target/recovery-next/release/cide-gui.exe` y `cide-tui.exe`.
Se conservan los ejecutables anteriores. Depósitos personales intactos en pruebas.
No se modifican archivos originales salvo Guardar explícito. Siguen abiertos la
integración GB, aceptación física/global, IME/DPI/a11y/clipboard real y M3–M5.
Sin dependencias nuevas, CI, push ni publicación.

El siguiente avance es histórico y queda subordinado a este cierre.

## Estado previo — marco GUI, ratón TUI y recuperación Rust, 2026-11-03

GUI ahora tiene marco de título integrado, barra de menús con el mismo fondo
`#2E3440`, pestañas redondeadas por grupo, selector de documentos abiertos y árbol
en píxeles con fuente de interfaz separada del código. Reutiliza una superficie
gráfica y el frame preparado en exposiciones. [Construcción](M2B_GUI_TABS_BROWSER.md).
TUI incorpora ratón y explorador modal de archivos/carpetas, con ayuda en español:
[uso y alcance](M2_TUI_MOUSE.md).

Ambas interfaces recuperan documentos sin guardar, bytes originales, undo/redo,
pestañas y vistas al salir y volver a abrir. El almacén Rust usa generaciones
inmutables, sincronización, verificación por relectura y HEAD/BACKUP atómicos;
si falla la confirmación, el editor permanece abierto. [Contrato y límites](SESSION_RECOVERY.md).
No hay todavía checkpoint periódico ni recolección de generaciones antiguas.

Check de todos los targets, fmt, Clippy sin advertencias y release offline aprobados.
Pruebas dirigidas de recuperación de core/archivos/aplicación/GUI/TUI, terminación
abrupta de procesos y reapertura ConPTY aprobadas; no corrida runtime global única.
GUI:15 checks de shell y13 de interacción, cero errores de pintura; TUI: navegador5,
ratón7 y recuperación4 comprobaciones en tres procesos; GUI también restauró cambios,
caret e historial en tres procesos reales ocultos. Fuentes fixture intactas.
Evidencia: `docs/evidence/gui-shell-browser-2026-11-03-*`.

Ejecutables actuales: `target/gui-next/release/cide-gui.exe` y `cide-tui.exe`.
Los anteriores `target/release` y `target/ui-es/release` se conservan.
La fuente paginada aislada leyó un fixture disperso de5GiB; el editor completo aún
no admite GB. Se eligió instantánea privada y falta integrarla con Document y los
índices progresivos: [auditoría](CORE_GIGABYTES_ADMISSION.md).
Aceptación física/global, IME/DPI/a11y/clipboard real y proveedores M3–M5 abiertos.
Sin nuevas dependencias externas, CI, push ni publicación.

Los estados siguientes son históricos y no sustituyen este avance.

## Estado vigente — cierre funcional TUI, 2026-11-03

Los bloques admitidos de TUI están implementados y validados localmente: regex,
comparación de abiertos, símbolos/includes/RUN léxicos opcionales, completado
textual, snippets, foco/overlays, clipboard Windows explícito y layout Unicode.
[Matriz vigente](M2_TUI_FINISH.md), [contrato Unicode](M2_UNICODE_LAYOUT.md).
530 nombres de pruebas aprobadas por suites/cierres dirigidos,18 casos nuevos;
dos helpers Git se ejecutan por sus padres, no una corrida global única.
Fmt/Clippy/release offline, cuatro flujos ConPTY y17 checks HWND propios aprobados.

Se preservan bytes/encoding/BOM/EOL/undo, F1/F2/F3 y un único modelo editable.
Unicode: grafemas/narrow/TAB4, caché512+256+128+128KiB/documento, filas64KiB,
presentación fuente compartida2MiB, pantalla131072 celdas. Clipboard TUI tiene
hilo Win32 lazy con buzón único y propietario que responde inactivo; pruebas sin
abrir clipboard personal. Las dos bibliotecas autorizadas están fijadas y el
mínimo declarado es Rust1.85; toolchain existente1.85.1, sin instalar programas.

La siguiente puerta es aceptación física/global con terminal/fuente del usuario,
clipboard real y mediciones RAM/arranque/pantalla; no se cierra con modelo/ConPTY.
No ampliar automáticamente a AVM/semántica M3–M4, Git escritura/CI M5, plugins
externos post-M5 ni gestos de ratón sin admisión. El paquete de desarrollo y sus
hashes están en los metadatos Unicode. Sin CI, push o publicación.

Las secciones fechadas siguientes conservan decisiones/evidencia históricas;
sus antiguos «siguientes» y «pendientes» se subordinan a este estado vigente.


Los hitos son secuenciales por evidencia, no por fechas.

**Revisión 2026-11-01:** la [auditoría de JC-IDE y ecode](AUDITORIA_ECODE_ARQUITECTURA.md) introduce una puerta de arquitectura antes de ampliar M2. Sublime Text continúa como referencia visual; ecode/eepp aporta patrones de operación, no un motor que se vaya a portar completo. La funcionalidad ya probada se conserva, pero no acredita rendimiento ni cierre de los defectos descubiertos.

## M0 — Base verificable

Núcleo byte-preserving, undo/redo, perfiles multilenguaje, puente Progress inicial, documentación y pruebas de base.

## M0.5 — Contrato de interacción

**Alcance:** formalizar la interacción antes de dibujar la TUI.

**Criterios de aceptación:**
- registro único de comandos y condiciones de disponibilidad;
- keymap contextual sin duplicados dentro del mismo scope;
- menús que derivan sus shortcuts del keymap;
- foco primario Editor/Sidebar/BottomPanel y overlays apilados;
- `Esc`, `F6`, pestañas, panel inferior y diálogos cubiertos por pruebas;
- shortcuts de Progress deshabilitados fuera de ABL/perfil;
- especificación normativa en `docs/TUI_INTERACTION.md`.

## M1 — Archivo y texto interpretado — cierre histórico de alcance

**Alcance cerrado:** interpretación explícita de fuentes sin convertir el almacenamiento canónico, guardado seguro, posiciones textuales, cambios externos y límites del historial.

**Criterios cumplidos:**

- UTF-8 se reconoce por BOM o validez; un archivo ASCII se marca como compatible/ambiguo y no como prueba de una code page concreta.
- Un archivo legacy que no sea UTF-8 **no se adivina**. El proyecto/documento debe seleccionar explícitamente Windows-1252, ISO-8859-1 o IBM850; el catálogo es ampliable.
- Decodificar nunca modifica los bytes canónicos. Secuencias inválidas se muestran como incidencias con su rango de bytes.
- Insertar texto usa encoding estricto: un carácter no representable produce error y no altera el documento.
- CRLF, LF, CR, mezcla de finales y presencia/ausencia de newline final se inspeccionan sin normalización.
- Existe mapeo comprobado entre offset de byte y posición lógica línea/columna, incluyendo UTF-8 multibyte y CRLF.
- `FileSession` detecta contenido modificado o eliminado externamente antes de guardar.
- Guardado existente usa temporal en el mismo directorio + flush/fsync + reemplazo atómico: `rename` en Unix y `ReplaceFileW` en Windows.
- Archivos nuevos usan commit no-clobber; un nombre creado por otro proceso no se sobrescribe silenciosamente.
- Un fallo durante la escritura deja intacto el archivo original.
- Undo/redo queda limitado a 1024 pasos por defecto, configurable incluso a cero; la piece table expone compactación explícita del buffer añadido.
- Matriz Windows/Ubuntu validada con tests, Clippy sin warnings, release build y demo.

La página de códigos **real de cada proyecto de producción** no se codifica globalmente en JC-IDE: se registra en el perfil del proyecto. Esto evita convertir una incertidumbre de configuración en una heurística destructiva. Véase [M1_FILES_TEXT.md](M1_FILES_TEXT.md).

El cierre histórico no cubre todas las rutas integradas de M2. La auditoría reproduce navegación dentro de CRLF y retención de bytes no limitada por el número de undos. Se corrigen en F1/F2, conservando las pruebas de archivo/encoding y sin describir M1 como certificación de escalabilidad.

## M2-F — Fundamentos antes de ampliar funciones — PRIORIDAD ACTUAL

**F0, completado en esta revisión:** evidencia estática, reproducción de fallos, baseline local y arquitectura de destino documentada. El ejemplo `audit_baseline` no certifica corrección ni modifica el editor. **F1 implementado y validado localmente:** propiedad única de estado de vista, identidades separadas, lotes atómicos y correcciones de apertura/cierre/CRLF. **F2 implementado y validado:** rangos/segmentos, proyección por revisión/encoding, checkpoints y EOL incrementales, render visible, cuotas y recolección conservando historial. [Mediciones, elección de bloques y límites](F2_RANGES_INDEX_MEMORY.md). F3 implementado con evidencia local de presentación de solo lectura, terminal incremental y trabajos internos acotados/versionados; [método y límites](F3_PRESENTATION_TASKS.md). Objetivos globales de RAM/arranque/tecla-a-pantalla abiertos; ConPTY mide inyección-a-VT, no pantalla física. El [checkpoint F1](F1_DOCUMENT_VIEWS_TRANSACTIONS.md) precisa APIs, política de posiciones y diferencias entre el catálogo TUI y las vistas por grupo.

| Incremento | Entrega concreta | Condición de salida |
|---|---|---|
| F1: propiedad y cambios — implementado | `DocumentId`/`ViewId`/grupos, un estado por vista, `EditBatch` atómico, `ChangeSet` completo, transiciones explícitas de apertura/cierre | Retirar cursor/scroll espejo y mutación que evita notificaciones. Un undo revierte replace-all; fallo de lote no altera nada; CRLF no admite caret interno; encoding legacy permanece seleccionable; dirty/reabrir coherentes; dos vistas comparten contenido/undo y conservan selecciones. |
| F2: acceso y memoria — implementado con mediciones locales | Segmentos/snapshots, índice textual por revisión, cachés con cuota, presupuesto de undo por bytes | Navegación y lectura de viewport estable sin copia/decode global. Comparación de estructuras bajo la misma carga; demostrar escalado y liberación de bytes no referenciados sin perder historial prometido. |
| F3: render y tareas — implementado, aceptación global de rendimiento abierta | Modelo visible de solo lectura, diff de celdas, loop con eventos de jobs, resultados versionados/cancelables | Dibujar no muta Workbench; UI recibe resultados sin otra tecla; tareas obsoletas no pisan estado; pantalla/listas virtualizadas; medición de latencia completa y cancelación. |

Cada incremento sustituye y retira su ruta antigua; no mantener dos modelos finales ni añadir sincronizaciones compensatorias. Conservar core byte-preserving, guardado, perfiles, contratos de interacción y regresiones útiles. Los nuevos tests deben cubrir comportamiento y fallos reales, no reflejar internals.

Dependencias: F1 → F2 → F3 → ampliación funcional M2A. El prototipo de backend GUI M2B puede empezar al estabilizar F1/F2 y madurar con F3; no depende de haber implementado toda la semántica ABL. No usar servicios ni índices permanentes para compensar lentitud del camino básico de edición.


## M2A — TUI funcional — EN DESARROLLO

**Alcance de productividad ABL:** la edición y el snippet engine son universales; el contenido ABL y sus servicios especializados pertenecerán a una extensión de lenguaje interna y opcional. Tokenización/formateo se prepara en M3; firmas, USING y herencia en M4. Ver [arquitectura de extensiones](EXTENSION_ARCHITECTURE.md) y [comandos ABL](ABL_EDITOR_COMMAND_SCOPE.md).

**Avance S1 del 2026-11-02:** búsqueda literal de proyecto, filtros, snapshots abiertos, lotes/generaciones/cancelación, navegación y preview/reemplazo por archivo implementados con evidencia local. Abiertos usan un undo y no se guardan automáticamente; cerrados usan FileSession, con conflictos explícitos. [APIs, cuotas, mediciones y límites](S1_PROJECT_SEARCH.md). No cierra S0 (case folding Unicode completo, búsqueda de documento acotada/regex real), matriz funcional ni aceptación global de rendimiento. El tratamiento de guardado Windows 32/33/1175 y recuperación 1176/1177 está documentado en [SAVE_WINDOWS_CONTENTION.md](SAVE_WINDOWS_CONTENTION.md).

**Avance S0 del 2026-11-02:** búsqueda literal cooperativa por rangos, default full case folding Unicode 17, palabra Unicode, selección/historial/marcadores y Go to escalar implementados. Un matcher compartido sustituye el scan síncrono ASCII y el KMP separado de proyecto. [Evidencia y coste](S0_DOCUMENT_SEARCH.md): suite de 186 pruebas más regresiones dirigidas de vistas, límites efectivos y recorridos Windows. La subida inicial de ~35 % del p95 de proyecto se abordó con decoder prestado, scratch reutilizado y rechazo ASCII en el mismo matcher: [bloque medido](S0_DECODER_OPTIMIZATION.md). En el corpus acordado p95 final 18,649 ms frente a 40,930 ms contemporáneos y 27,457 ms S1; cancelación sin mejora consistente. Suite final completa de 191 pruebas y recorridos nativos. Regex con presupuesto real, matriz manual y aceptación global siguen abiertas.

**Avance de evidencia M2 del 2026-11-02:** [corpus ampliado y procesos](M2_ACCEPTANCE_EVIDENCE.md), 194 pruebas completas, Unicode/legacy, 4097 piezas y ocho documentos/dos vistas. Se corrigió la materialización completa del layout frío: pico real de línea ASCII de 16 MiB 434,9 → 18,0 MiB. Arranque con caché del SO no vaciada p95 0,064/0,114/0,204 s (siete procesos por forma). Modelo de ocho documentos con líneas muy cortas aún ~69,5 MiB privados; metadatos densos y aceptación física/global permanecen abiertos.

**Search Plus es prioridad de producto:** primero documento activo, luego búsqueda en proyecto cancelable, más tarde referencias apoyadas en compilador XREF y catálogo de esquema. Ver [plan Search Plus](M2_SEARCH_PLUS.md).

Esta prioridad se ejecuta sobre M2-F: antes de sumar búsqueda de proyecto deben existir transacciones, resultados por revisión y presupuesto de trabajo/memoria. Navegación de proveedores, Git y asistentes no se anteponen a esa base.

Atajos acordados: Ctrl+D duplicar, Ctrl+Shift+D eliminar línea, Tab/Shift+Tab sangría, Ctrl+/ comentar/descomentar, Ctrl+Shift+/ bloque. La selección columna requiere rangos rectangulares reales y fallback por Palette cuando el terminal no entrega combinaciones. No confundir código incorporado con funcionalidad ya validada.

Existe `jc-tui` con ejecutable `jc-ide-tui`, entrada de terminal mediante crossterm y contrato de aplicación compartido. La presencia de código o pruebas parciales no equivale a aceptación funcional completa. Implementar y verificar cada flujo de extremo a extremo antes de cerrar este hito.

**Checkpoint actual:**
- edición real sobre `Workbench`, sin buffer/undo paralelo;
- Explorer de proyecto navegable y restringido a su raíz;
- resultados de búsqueda navegables desde el panel inferior;
- Duplicate/Delete Line/Move Line/Toggle Comment implementados como comandos reales;
- menús, palette, foco y pestañas ya consumen el contrato central;
- CI remoto permanece dormido hasta M5; este checkpoint no se declara validado por Actions.

**Incremento M2A — dos grupos funcionales (2026-11-01):** `PaneManager` ya está conectado al renderer TUI, comandos, menús y palette. Cursor, selección y viewport se conservan por grupo; las ediciones/undo/redo ajustan posiciones de la otra vista e historial sin duplicar documentos. Incluye pin, apertura lateral, back/forward, scroll vertical vinculado y fallback al grupo activo con poco ancho. Hay pruebas locales y smoke test en terminal Windows; véase [VALIDACION.md](VALIDACION.md). Siguen pendientes símbolos/includes, Git read-only/diffs y revisión manual completa. Git con escritura permanece en M5. Ver [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).

**Pendiente para cerrar M2A:** aceptación global de rendimiento de M2-F, picos/sufijos restantes y ampliar frío/teclado sobre el corpus ya medido, regex real si se incorpora y matriz Search Plus, integración de proveedores reales sobre el contrato de tareas F3 y revisión manual completa de teclado. El incremento literal de proyecto con preview y conflictos ya existe; no se confunde con cierre completo de Search Plus. El contrato de tareas/Problems/Output ya tiene proveedor de prueba y regresiones locales; no equivale a Progress conectado. La ejecución real de Progress y su validación pertenecen a M3; no se aplazan defectos conocidos de datos/edición hasta M5.

**Criterios de aceptación:**
- uso completo por teclado;
- abrir/guardar/cerrar, pestañas, búsqueda, sidebar, panel inferior y command palette;
- toda acción de negocio resuelve un `CommandId`;
- menús muestran el shortcut real;
- el contrato de tareas entrega diagnósticos sin bloquear edición y descarta resultados obsoletos; Progress Check/Compile/Run/Schema permanecen deshabilitados si no hay proveedor validado;
- el frontend no crea un segundo buffer ni su propio undo.

**Bloque posterior de RAM/teclado:** [metadatos compactos](M2_DENSE_LINE_METADATA.md), recolección que reutiliza arenas enteras y seis regresiones de teclado; 209 pruebas completas y recorridos nativos. Modelo denso ~49,3 MiB tras frío; pico nativo 71,9 → 51,7 MiB, por lo que la puerta global permanece abierta. Frío/sufijos tienen costes registrados. El dispatcher TUI ya impide ejecutar menús deshabilitados; [matriz y pendientes físicos](M2_KEYBOARD_MATRIX_AUDIT.md).

**Preparación paralela autorizada del 2026-11-02:** [DiffModel puro](DIFF_MODEL_READ_ONLY.md) de fuentes inmutables, hunks y correspondencia de líneas exactas, con cuotas/cancelación; seis regresiones y suite integrada de 200 pruebas. No hay comandos/renderer/proveedor Git conectados ni rendimiento diff aceptado. Los picos/sufijos densos restantes y las puertas globales de M2-F siguen prioritarios.

**Proveedor read-only posterior:** [Git HEAD/index](GIT_READ_ONLY_PROVIDER.md), Windows con snapshots de bytes exactos/identidad, límites y contención/cancelación. Nueve regresiones efectivas y suite completa de 218 pruebas; composición con DiffModel verificada. Aún no hay entrega por revisión/generación, UI/status ni comandos Git funcionales. Su RAM/latencia auxiliar siguen por medir antes de aceptación global. Escrituras Git permanecen M5.

**Integración posterior del 2026-11-02:** [entrega Git versionada](GIT_DIFF_DELIVERY.md) y [visor TUI](GIT_DIFF_PRESENTATION.md) conectados para el archivo activo. HEAD/index se comparan con el snapshot del buffer sucio mediante el JobSystem existente; un trabajo en vuelo, pendiente de metadatos, cuotas y descarte por revisión/encoding/vista/ruta/generación. Comandos centrales de configuración, refresh y cancelación, lista de hunks, navegación y diff unificado visible por rangos. 237 pruebas locales aprobadas al cierre, fmt/Clippy/release y recorrido ConPTY HEAD/index sin otra tecla, stale/undo y conservación de disco/index. La ronda inicial detectó una expectativa antigua de menú; se corrigió y repitió solo TUI. No acredita status de repositorio, watcher, otras plataformas ni aceptación global RAM/pantalla; auxiliares Git aún por medir. Las menciones anteriores sin UI/jobs describen checkpoints históricos.

**Bloque siguiente medido:** [sufijos y crecimiento acotado](M2_SUFFIX_COST_REVIEW.md), 4914 observaciones antes y otras 4914 después, 240 pruebas completas y fmt/Clippy/release. Una primera newline ya no duplica el índice: capacidad posterior de proyección en el corpus de 8 MiB, 8388792→4227256 B. Se conserva el coste O(L) y el caso frío único 1760,8→2145,2 µs; no se declara mejora general de latencia/RAM. [Auxiliares Git](GIT_AUXILIARY_MEASUREMENT.md): nueve sesiones, sumas observadas por ciclo hasta 11,156 MiB privados en el corpus cercano a 2 MiB, excluyendo bootstrap/conhost inicial/harness; cobertura temporal limitada. Siguientes frentes activos: distribución de primeras ediciones y recorrido nativo denso con edición/undo en ocho documentos. No se cierra M2A ni la puerta global de 50 MiB/pantalla física.

**Cierre de esos dos frentes:** [tres pares de distribución fría](M2_COLD_GROWTH_DISTRIBUTION.md), 5580 filas, mismo ejemplo sobre `6cf6f13` y `d6de969`; y [14 procesos nativos retenidos](M2_DENSE_EDIT_EVIDENCE.md), mismo recorrido de edición/undo en ocho documentos. Pico histórico de compromiso 68,160→52,461 MiB; máximo privado observado tras editar 65,922→50,434 MiB. La puerta global sigue abierta. El p95 frío no cambia consistentemente de signo entre pares; no se declara aceleración ni regresión general. Sin cambio productivo ni repetición de suites sin necesidad. Ese bloque experimental se cerró en `191bd82`; su selección e integración posterior están documentadas a continuación.

**Bloque por bloques integrado y medido:** [diseño/evidencia](M2_BLOCK_LINE_INDEX.md),
una única representación productiva, retirado el comparador dependiente del plano
(reproducible en `191bd82`). Suite completa 245 aprobadas y revisión de capacidad
posterior con 15+3 pruebas afectadas: 246 distintas al cierre; fmt/Clippy/release.
Dos pares de Workbench con igual ejemplo, 48 procesos nativos / 42 retenidos y
1984 operaciones de pipeline. Pico histórico denso editado 52,461→43,727 MiB,
repetición 52,313→43,820 MiB. El movimiento caliente sube ligeramente; sparse/cold
tienen tradeoffs. [Undo por etapas](M2_UNDO_PIPELINE_PROBE.md): mejora interna en
ambos órdenes; p95 VT repetido 16,9702 ms plano / 16,9147 ms bloques, sin persistir
la diferencia inicial. Global RAM/servicios/pantalla física y M2A siguen abiertos.
Conservar evidencia, sin repetir suites por documentación ni cambios menores.

## M2B — GUI Windows mínima — EN CONSTRUCCIÓN; primer ejecutable validado

Ya existe un [estudio estático de escena y contrato](M2B_SCENE_STUDY.md) preparado en paralelo: ABL/include PIN/Explorer/Search/palette, datos locales y matriz neutral. Sus comprobaciones estáticas pasan; la captura headless falló sin imagen, por lo que revisión renderizada sigue pendiente. No es editor gráfico ni decisión de backend.

Elegir tecnología gráfica con mediciones y consumir el mismo `jc-app`. Iniciar una escena de comparación al estabilizar F1/F2: fuente ABL + include en referencia anclada + Explorer + búsqueda + palette. Mantener la dirección visual de Sublime Text: editor dominante, marco compacto, tabs discretas, sidebar colapsable, panel inferior y barra de estado. Estilo visual no implica copiar shortcuts ni compatibilidad de plugins.

**Criterios de aceptación:** abrir/editar/guardar, mouse, portapapeles, IME, accesibilidad, pestañas y paneles sin duplicar comandos ni lógica de aplicación. La GUI consume los mismos stores/grupos de F1 (Split, Pin, navegación y posteriormente DiffModel), añadiendo Ctrl+Click y drag como entradas. Capturas y pruebas de foco/DPI 100/150/200 %, fuentes de ancho variable por grafema, dos vistas del mismo documento y comparación de latencia/RAM/arranque. Rechazar un backend que obligue a normalizar bytes o mantener otro buffer/undo editable. Minimap después del render visible eficiente, sobre resumen cacheado.

## M3 — Entorno Progress funcional

**Avance 2026-11-11.** GUI y TUI comparten los trabajos ABL de comprobación,
compilación y ejecución por lotes. OpenEdge 11.7 validó PROPATH ordenado,
inclusiones anidadas, diagnósticos en Problemas, captura en Salida y
cancelación. El segundo bloque validó un perfil `.pf` con dos bases de
desarrollo aisladas y aliases reales. GUI y TUI actualizan y reabren el
catálogo de esquema; una falla conserva la última lectura válida.
El perfil puede fijar la versión del AVM y **Salida** registra la versión
efectiva. Los recorridos de validación del perfil ERP están preparados para
ambas interfaces. Queda ejecutar las pruebas con OpenEdge 10.0B y con un
perfil autorizado del ERP de desarrollo.
La revalidación de Windows con OpenEdge 11.7 pasó 22 pruebas dirigidas
en el puente, la GUI y la TUI. Los 12 archivos de las bases de prueba
conservaron su SHA-256.
[Configuración y evidencia](PROGRESS_BRIDGE.md)

Por prioridad del usuario, se adelantó la primera integración LSP de GUI y TUI.
La navegación semántica para C#, JavaScript/TypeScript, PHP y Java quedó en
`2bfb887`. El incremento de autocompletado y comprobación del archivo activo
en Problemas pasó validación local con esa misma configuración. [Uso y alcance](M3_LSP_NAVEGACION.md)

Estabilizar el contrato interno de capacidades para vincular proveedores de lenguaje sin introducir una runtime genérica de plugins; comenzar el paquete integrado opcional `progress.abl`, manteniendo el puente existente `jc-progress`. Validar OpenEdge 10.0B y 11.7, includes, PROPATH, aliases, code pages, compilación y ejecución en contexto real. Resolver includes `{...}` y RUN estáticos desde el perfil del proyecto sin ejecutar el programa para navegar; señalar destinos dinámicos como inciertos. La AVM solo se ejecutará a petición del usuario. Conectar Check/Compile/Run/Schema a Problems/Output a través de los jobs de M2-F; comprobar cancelación y que resultados de otra revisión no se apliquen silenciosamente.

## M4 — Esquema y asistencia ABL

**Cerrado localmente en Windows con OpenEdge 11.7** para los flujos
admitidos. La evidencia y los límites se detallan en el
[puente Progress](PROGRESS_BRIDGE.md#validación-local-de-m4).

El primer bloque comparte en GUI y TUI el autocompletado de tablas, campos e
índices del catálogo del proyecto. [Uso y alcance](PROGRESS_BRIDGE.md#autocompletado-de-esquema-en-gui-y-tui).
El segundo amplía las sugerencias a buffers estáticos, tablas temporales y
declaraciones en inclusiones por PROPATH, con comprobación en OpenEdge 11.7.
**Ir a definición** usa ese análisis para localizar declaraciones en el
documento y en inclusiones, con apertura normal o lateral en GUI y TUI.
**Buscar referencias** sigue el ámbito del símbolo y encuentra consumidores
del mismo proyecto que resuelven la misma declaración incluida por PROPATH.
**Esquema** lista esas declaraciones en GUI y TUI, con apertura lateral.
**Buscar referencias con XREF** usa el compilador para añadir referencias a
tablas, campos e índices de bases conectadas, incluidas ubicaciones en
inclusiones expandidas por el AVM.
**Ir a definición** resuelve tipos ABL importados con `USING` y abre la
declaración del `.cls` correspondiente por PROPATH en GUI y TUI.
La navegación de métodos y propiedades sigue `INHERITS` con `SUPER`,
`THIS-OBJECT` y receptores de tipo declarado. Selecciona sobrecargas
cuando hay una firma inequívoca, incluidos los modos `INPUT`, `OUTPUT`
e `INPUT-OUTPUT` con tipos primitivos conocidos. También sigue cadenas
de propiedades y retornos declarados mediante PROPATH.

La ruta del símbolo activo usa el Esquema versionado del documento. En GUI
se muestra dentro de Esquema. En TUI se integra al encabezado del grupo
activo. Ambas interfaces abren la declaración en el grupo actual o al lado.
Mover el cursor reutiliza los ámbitos calculados para esa revisión.

Las acciones de edición permiten ordenar `USING` explícitos, sincronizar
prototipos `FORWARD`, generar constructores del padre directo y preparar
sobrescrituras e implementaciones de clases e interfaces. Comparten vista
previa, confirmación y un solo paso de deshacer. El catálogo, XREF, imports y
jerarquía son aportaciones ABL. La navegación y la transacción de edición
permanecen en el editor compartido.
[Uso](PROGRESS_BRIDGE.md#acciones-de-edición-abl) · [Alcance](ABL_EDITOR_COMMAND_SCOPE.md#c--m4-funciones-que-requieren-semántica).

La generación de una interfaz de servicio depende de un contrato real de
integración. El [renombrado entre archivos](ABL_PROJECT_RENAME.md) ya cubre
declaraciones compartidas por inclusiones ABL encadenadas resueltas por PROPATH
en GUI y TUI y llamadas indirectas con destino literal y handle comprobado.
Los cambios se recuperan y se deshacen como grupo. Quedan por ampliar los
contextos de preprocesador dinámico y los destinos calculados en ejecución.

## M5 — Integración y validación local

**Avance en Windows.** En una corrida transversal pasaron 975 pruebas
con tres fallos. Las tres regresiones pasaron al repetir sus casos
dirigidos, junto con la nueva prueba del comando Git con `PATH` vacío.
También pasaron formato, Clippy, las pruebas TypeScript con servidor
real y la recuperación en los ejecutables extraídos del MSI. El
identificador de menú de LSP ahora es independiente del de Markdown.
La compilación de desarrollo pasó las 152 comprobaciones nativas
en una ventana oculta después de fijar la geometría de los fixtures.
El MSI optimizado 0.1.1 pasó ICE y la verificación de 666 archivos
extraídos. [Evidencia del instalador](../installer/README.md#validación-local-de-011).

**Avance en Linux.** En Debian 13 x86_64 (WSL) con Rust 1.88.0 pasaron
689 pruebas del workspace, con 22 ignoradas, el formato, Clippy con avisos
tratados como errores, la compilación optimizada y la demo de CLI. La TUI optimizada abrió
un archivo sintético en una terminal privada, salió mediante Archivo › Salir
y preservó el archivo byte por byte. Los casos de publicación del índice y
del clon local que dependen de primitivas Win32 conservan su cobertura en
Windows. En Linux se comprueba que preparar un archivo para el índice devuelva un error de
plataforma sin modificar el índice ni el contenido. La GUI nativa, WinHTTP
y la fuente paginada de archivos grandes siguen siendo de Windows.

Git completo (stage/unstage por hunk, commit, fetch, pull, push, ramas, merge y resolución BASE/OURS/THEIRS/RESULT), proveedores semánticos/LSP, tareas e integración con OmniCore, todos bajo demanda. Construir los flujos de Git con confirmaciones y protección de archivos modificados; el visor read-only se adelanta a M2A. Hardening del contrato de proveedores internos de lenguaje y sus contribuciones al registro central; búsqueda de proyecto permanece priorizada en M2A.

[Fusión local de ramas](GIT_LOCAL_MERGE.md): GUI y TUI revisan y confirman el
avance directo o la fusión divergente. La resolución de archivos ordinarios
muestra Base, Actual, Entrante y un Resultado editable antes de confirmar.
Las colisiones entre archivo y carpeta se resuelven eligiendo la estructura
completa de una versión.
Los renombres exactos en competencia se resuelven eligiendo la ruta de una versión.

La validación sigue siendo **local** en M5 mediante
[`scripts/check-local.py`](LOCAL_VALIDATION.md). GitHub Actions requiere
una autorización expresa para la ejecución concreta.

1. Ejecutar la suite del editor en Windows y la matriz de formato, pruebas,
   Clippy y compilación para los componentes admitidos en Linux.
2. Actualizar el MSI y verificar sus archivos, la actualización desde el
   paquete anterior y la recuperación de sesiones en GUI y TUI.
3. Reutilizar el motor Git en OmniCore y OmniCoder, con contratos públicos
   de lectura y pruebas en cada host. CIDE ya tiene operaciones locales y
   transporte embebido. Los dos hosts ya tienen una integración opcional
   probada para leer `HEAD`. [Contrato y recorridos](GIT_SHARED_CORE.md#consulta-de-head-para-otros-hosts).

## Anexo post-M5 — Compatibilidad con Sublime Text

Después de cerrar M5 se abrirá un trabajo separado para compatibilidad con plugins, extensiones y paquetes de Sublime Text. **No forma parte de M5 y no debe condicionar M2–M5.**

La estrategia será incremental: primero formatos declarativos de bajo riesgo, después carga de paquetes y finalmente una capa de compatibilidad controlada para plugins Python. El objetivo no es clonar Sublime Text ni prometer compatibilidad binaria/API total, sino reutilizar una parte material de su ecosistema sin romper la arquitectura de JC-IDE.

Especificación: [POST_M5_SUBLIME_COMPAT.md](POST_M5_SUBLIME_COMPAT.md).

## Presupuestos y medición desde M2

- editor base: objetivo inicial <50 MiB de memoria privada;
- arranque: objetivo inicial <0,5 s hasta poder editar;
- Progress y otros procesos: medir por separado y como total;
- archivos grandes: medir latencia, piezas, historial y caches antes de optimizar.

Son objetivos exploratorios. La medición local empieza en M2-F y continúa en M5. El perfil actual `opt-level="s"` prioriza tamaño. La política de release se fija comparándolo con 2/3 bajo las mismas cargas.

Metas iniciales para un equipo de referencia identificado en cada corrida: p95 de movimiento/consulta textual <2 ms con índices calientes en archivos de hasta 4 MiB; p95 tecla-a-pantalla <16 ms con viewport fijo, una/dos vistas y servicios inactivos; cancelación de búsqueda perceptible <100 ms en la carga local. Son presupuestos para orientar F2/F3, no garantías universales. Ajustarlos solo con evidencia registrada; informar degradación en archivos/líneas extremas.

Matriz local: 4 KiB, 64 KiB, 1/4/16 MiB; líneas cortas y una línea larga; UTF-8 multibyte/combining/ancho doble, legacy, BOM y EOL mixtos; pocas/muchas piezas; búsquedas con cero/muchas coincidencias; edit/undo/redo y cerrar/reabrir. Registrar hardware, OS/toolchain/perfil, corpus, calentamiento, muestras y p50/p95; separar apertura fría, edición caliente y procesos auxiliares.

Medir también bytes decodificados, asignaciones por operación, memoria privada y retención tras liberar historial/cerrar documentos. Un viewport fijo no debe provocar trabajo proporcional a todo el archivo en cada tecla. Cuotas de workers, cola, resultados y cachés serán configuración explícita con valores derivados de esas mediciones. La primera evidencia y sus límites están en [AUDITORIA_ECODE_ARQUITECTURA.md](AUDITORIA_ECODE_ARQUITECTURA.md).


## Prioridad vigente: CPU y concurrencia acotada — 2026-11-02

Juan pide aprovechar núcleos disponibles. El bloque Git proveedor/controlador
está cerrado con 261 pruebas completas, fmt/Clippy/release; su presentación TUI
se pospone ante esta prioridad. Leer GIT_STATUS_READ_ONLY y GIT_STATUS_DELIVERY.

Primero inventariar hardware real y perfilar búsqueda por archivos, diff,
decodificación y tareas de fondo. Elegir el cuello medido; reutilizar planificador
cuando encaje, concurrencia configurable/acotada, cancelación, cuotas/backpressure,
resultados deterministas/versionados, sin pools anidados ni copias grandes.
Preservar bytes/encoding/undo y no paralelizar escrituras de un documento.

Antes/después con corpus/protocolo idénticos: throughput, latencia/p95 del tramo
medible, CPU por proceso/tiempo y memoria/picos. 100% CPU no es aceptación.
Reservar margen para Qwen; sin afinidades/prioridades/servicios/GPU ajenos ni
archivos privados. Si no puede verificarse su actividad sin interferir, registrar
carga concurrente desconocida y no atribuir toda variación al cambio. Máximo
tres agentes, un integrador, compilaciones y mediciones pesadas serializadas.
Construir bloques completos antes de validar. Sin instalaciones, push ni CI.


## Cierre CPU y siguiente bloque — 2026-11-02

CPU implementado y medido: [CPU_PERFORMANCE](CPU_PERFORMANCE.md). Search Plus usa
un solo engine por archivo, default dos trabajadores incluido coordinador,
configurable 1…4 con admisión/headroom conservadores. TUI ya acepta
`--search-workers 1..4`; uno reduce demanda mientras se trabaja con otras cargas.
Se conservan bytes/encoding/undo, revisiones, cancelación y cuotas globales; helpers
lazy reutilizados sin snapshots duplicados ni pools anidados/F3 documental ficticio.

Cuatro capturas / 1488 consultas conservadas, mismo probe/corpus y dos órdenes.
16 MiB: mediana 1,24–1,49× más rápida, uso agregado ~1→1,73–1,80 núcleos equivalentes;
CPU total aumenta 30–48%. Memoria privada muestreada del harness baja de
5,527–5,625 a 3,520–4,230 MiB, incluido reader sin crecimiento para guardia EOF.
Un caso pequeño tuvo mediana peor; no es aceleración universal ni menos CPU total.
Codegen jc-app s/2/3 se comparó aparte: se **mantiene s**, sin mejora UTF-8 consistente
que justifique cambiar política global. Qwen/carga, energía y temperatura no
observados/controlados; no tocar procesos ajenos. Pantalla física/global RAM abiertas.

Validación CPU: 269 pruebas distintas al completar las suites tras corregir dos
oráculos S1 sensibles a mayúsculas; luego integración de arranque TUI, 47 pruebas
TUI/CLI aprobadas con cinco nuevas, total **274 distintas**. Formato, Clippy
workspace/all-targets del executor y dirigidos posteriores, release y CLI real
pasaron. No una suite completa de 274 en una sola corrida. No repetir perfiles
ni suites por documentación; sin instalaciones/CI/push/afinidades/prioridades.

**Siguiente pendiente autorizado:** presentación TUI del estado Git de repositorio,
sobre el proveedor y controlador ya implementados en `16071e3`; leer
GIT_STATUS_READ_ONLY/GIT_STATUS_DELIVERY antes de conectar. Preservar paths byte
opacos, rechazo de filtros, configuración aislada, generación/opciones vigentes,
lectura visible y ausencia de escrituras. No prolongar optimizaciones CPU sin
otra pregunta concreta ni activar un incremento ajeno al plan.

## Cierre del siguiente incremento: Git status TUI — 2026-11-02

La presentación TUI pendiente en el cierre CPU ya está conectada al controlador
existente: consulta explícita de repositorio sin documento, rutas byte opacas,
filas visibles acotadas, selección, refresh/cancel y configuración con intención.
Entrega vigente solo por poll_tasks, composición de solo lectura y recuperación
tras pánico incluso obsoleto. No watcher ni rutas editables o escrituras Git.

Bloque integrado antes de validar: 290 pruebas completas aprobadas, fmt,
Clippy workspace all-targets y release bins/examples. [Contrato/uso y límites](GIT_STATUS_PRESENTATION.md)
y [evidencia](VALIDACION.md). CPU queda cerrado con sus datos existentes; no se
repiten perfiles ni pruebas por ajustes documentales. Las menciones anteriores
de presentación status pendiente describen checkpoints históricos. Navegación
segura desde paths Git, otras funciones M2 y aceptación global de RAM/pantalla
siguen pendientes; no se declara cierre de M2A ni se activa otro incremento aquí.

## Rectángulo de una acción y siguiente bloque activo — 2026-11-02

Implementado el [rectángulo acotado](M2_RECTANGULAR_SELECTION.md): selección real
por celdas, TAB4/virtual, clipboard interno rectangular, copy/cut/paste/entrada/
Delete/Backspace con transacción única, comandos y Palette, paint readonly.
No es cierre del modo columna Unicode general: rechaza grafemas/anchos no
admitidos; las ediciones efectivas terminan el rectángulo. No OS clipboard ni GUI.

Cierre local: 310 pruebas distintas aprobadas en cierre selectivo después de
corregir un acceso que exponía también selección lineal; fmt/Clippy/release.
No repetir pruebas aprobadas ni perfiles por documentación. El siguiente pendiente
M2A está activo: revisión/contrato de unir y dividir líneas genéricos del anexo
ABL_EDITOR_COMMAND_SCOPE. Join usará rangos de separadores/indentación y un batch
F1; Split reutilizará insert_newline/preferred EOL. No ampliar a AVM/GUI, Unicode
general ni otra optimización CPU sin una necesidad concreta.

## Join/Split cerrados y siguiente admisión activa — 2026-11-02

Completado [unir/dividir líneas](M2_LINE_COMMANDS.md): modelo universal,
transacción única, lectura de prefijos/EOL por rangos y clasificación byte→línea
sin layout escalar. Split reutiliza la inserción de newline existente; Enter se
centraliza y Ctrl+Shift+J no interfiere con Ctrl+J. Preservados BOM/EOL final,
encoding/bytes fuera de cortes, vistas y undo. No normalización sintáctica.

Validación: 74 dirigidas de app y cobertura TUI completada tras corregir solo un
fixture de foco; nueve nuevas, 319 distintas al cierre, fmt/Clippy/release.
No una suite única319 ni una repetición de perfiles. Contrato TUI se reconcilia
con las funciones implementadas y límites actuales, sin inferir recorrido físico.

**Siguiente bloque activo:** auditoría/admisión de regex real acotado de Search
Plus, prioridad universal pendiente de M2A. Revisar engine compartido y contrato
de bytes/Unicode, presupuesto/cancelación, capturas/reemplazo y costo sin instalar
dependencias. No habilitar el switch hasta que haya un motor operativo y validado;
no adelantar proveedores de extensión/AVM M3 ni GUI sin ventana autorizada.

## Admisión regex completada: decisión de dependencia pendiente

La auditoría siguiente ya está recogida en [M2_REGEX_ADMISSION](M2_REGEX_ADMISSION.md).
No se habilitó regex ni se descargó nada. Candidato para evaluar: regex-automata
0.4.18 mantenido, con compilación/VM cooperativa, presupuestos, coordenadas y
reemplazos explícitos; no asumir que una llamada monolítica es cancelable.
Lock/cache no contienen un motor. La instrucción de continuidad «sin instalar
dependencias» exige decisión de Juan antes de añadir/fijar esa dependencia Rust.
Alternativa: conservar regex deshabilitado y seleccionar otro pendiente M2A.
No implementar un parser/engine casero para esquivar la restricción. La admisión
es diseño/revisión de fuentes primarias y código local, sin pruebas/benchmarks
nuevos ni aceptación de un motor; siguen vigentes los 319 casos del cierre previo.

## Prioridad vigente de Juan: construir M2A y M2B — 2026-11-02

Juan cerró el plan correctivo y pidió continuar el roadmap original: «Continúa
con la construcción, síguete con M2A y M2B»; aclaró que automatizamos hasta donde
sea posible y solo deja las pruebas de usuario para el final. Las puertas globales
RAM/pantalla quedan documentadas, sin bloquear construir funciones ni abrir
optimizaciones indefinidas. Regex permanece pendiente sin autorización para
agregar/descargar/fijar regex-automata u otra dependencia; no volver a pedirlo para
avanzar un pendiente independiente.

Frentes admitidos en el árbol actual: [Quick Open M2A](M2_QUICK_OPEN.md), reservado
Ctrl+P del roadmap, y [primer GUI Windows M2B](M2B_NATIVE_WINDOWS.md) mediante
Win32/GDI nativo y Workbench compartido. Dependencias del GUI: solo paquetes
existentes jc-app/jc-text, sin descarga de framework. El HTML de prototypes sigue
siendo estudio histórico, no el ejecutable. Un integrador, hasta tres agentes
totales, archivos exclusivos y validación pesada serial al cerrar bloques amplios.
No CI/push/instalaciones, cambios de seguridad/servicios ni Qwen/GPU.

Estado al activar: implementaciones en construcción/integración, sin anunciar aún
pruebas de este bloque. El GUI real tendrá smoke de HWND propio oculto y renderer
GDI; no controla otras aplicaciones. Validar compilación, comportamiento compartido,
reporte/bitmap y límites, y dejar checklist manual para Juan sin pedir prueba ahora.

## Primer cierre funcional M2A+M2B y siguiente bloque activo

Quick Open de proyecto TUI y GUI Win32/GDI inicial implementados: consulta
cancelable/acotada, revalidación/apertura confinada y sesión única; ventana real
con edición/selección/undo/grupos/guardar y frame readonly. [Uso/límites finder](M2_QUICK_OPEN.md)
y [GUI/evidencia nativa](M2B_NATIVE_WINDOWS.md).339 pruebas Rust distintas en
cierre selectivo +16 checks del ejecutable real, fmt/Clippy/release aprobados.
No cambios de dependencias externas, CI/push o perfiles repetidos.

M2B continúa en construcción: no declarar paridad completa ni puertas físicas
cerradas por ese smoke. El siguiente bloque ya admitido/construyéndose es adaptar
Quick Open y Command Palette al GUI, con selección/input acotados y presentación
GDI readonly, usando los controllers y comandos existentes. Una notificación al
loop nativo debe recibir completions sin otra tecla y sin polling permanente.
Regex permanece pendiente; no solicitar su dependencia para avanzar este frente.
Las pruebas manuales de usuario se conservan para el final.


## Cierre de selectores GUI y siguiente construcción M2B

Quick Open/Palette/Open Folder nativos implementados: [contrato](M2B_NATIVE_SELECTORS.md).
350 pruebas completas locales y23 checks del release real, Clippy/release aprobados.
Se conserva el único Workbench/undo y no hay hilo puente/indexador ni dependencia
nueva. M2B permanece abierto. Próximo bloque amplio: clipboard OS + elección de
encoding en GUI; pruebas locales al cerrar ambas rutas. Manual/IME/DPI/accesibilidad
y aceptación física/global siguen pendientes, sin posponer construcción funcional.


## Prioridad de las tres funciones — cierre funcional

Juan pidió construir juntas Mostrar invisibles, buscar bloques multilínea y
reemplazarlos, antes de nuevas superficies. Las tres están integradas en TUI/GUI:
[contrato, uso y evidencia](M2_INVISIBLES_MULTILINE.md). Mapa ASCII por spans fuente,
caret/selección/scroll propios de display, EOL exactos y campos navegables/pegables
con gestos distintos de ejecutar. Replace-all documental conserva una transacción.
También se integraron clipboard lineal nativo y elección gráfica de interpretación.

376 Rust distintos en cierre selectivo; Clippy/release y29 checks nativos pasaron.
No se infiere aceptación OS clipboard/IME/DPI/accesibilidad/pantalla física ni RAM
global. Regex/dependencias siguen aplazadas. Próximo pendiente funcional M2B:
flujo Save All explícito por documento, incluyendo Save As para nuevos y cancelación;
nunca guardar nombres untitled generados por un shortcut. Validar al cerrar ese
bloque, sin repetir las pruebas de este cierre por documentación.


## Save All GUI implementado; siguiente S1 gráfico

[Save All nativo](M2B_SAVE_ALL.md) ya resuelve nuevos con Save As/cancelación,
targets por revisión/path y resultados parciales explícitos, sin mover vistas.
6 pruebas nuevas,45 dirigidas y32 checks nativos aprobados; conjunto histórico382
distintos, Clippy/release aprobados. Las tres funciones prioritarias anteriores
siguen implementadas. Próximo bloque amplio M2B: Search Plus de proyecto en GUI
con controller S1 existente, entrada acotada y resultados versionados/solo lectura.
No nuevo motor/índice permanente, regex ni dependencies/CI. Construir antes de
otra ventana de tests; manual/aceptación global permanece abierta.

## CIDE y Search Plus de proyecto GUI — cierre 2026-11-02

Marca visible CIDE; comandos cide/cide-tui/cide-gui con aliases jc-ide compatibles.
Crates, raíz, remoto e historia no se renombran. Icono original JG+CIDE verde para
TUI/CLI; variante naranja para GUI, recursos PE de siete tamaños y HWND verificados.
[Inventario/contrato](CIDE_REBRAND.md).

[S1 gráfico](M2B_PROJECT_SEARCH_NATIVE.md) implementa campos Query/Include/Excludes,
worker configurable1..4, resultados acotados por notificación, apertura validada,
refresh, stale/cancel y cambio de raíz. Misma lógica S1; sin índice/motor nuevo.
391 casos distintos aprobaron por suites (260+131), fmt/Clippy/release y41 checks
nativos. ZIP local de desarrollo con manifiesto, sin instalación/publicación.
Quedan los flujos GUI de proyecto replace y diff/status read-only, proveedores y
puertas físicas/globales ya documentadas. No se cierra M2A/M2B ni se solicita prueba
manual intermedia; no repetir suites/perfiles aprobados para cambios documentales.

## Reemplazo y Git GUI integrados — 2026-11-03

Bloque amplio según instrucción nocturna de Juan: [reemplazo y Git readonly](M2B_PROJECT_REPLACE_GIT.md)
construidos/cableados antes del cierre integral. Entrada multilínea/preview,
confirmación, un archivo por turno, no-op, undo, conflictos/cancelación y resumen;
status de disco/index y diff HEAD/índice contra buffer sucio, navegación validada,
configuración explícita, delivery y pintura sin admisión de snapshots.

53 Rust distintos dirigidos y58 checks nativos aprobaron;8 nuevos, histórico399
(no corrida global399). fmt/Clippy/release aprobados, metadataGit sintética intacta.
Sin dependencias/CI/push ni perfiles repetidos. M2A/M2B y gates físicos/globales
permanecen abiertos. Próximo frente M2B activado: contrato del Explorer lateral
gráfico sobre catálogo lazy compartido/acotado; respetar raíz, dirty, pin y vistas.
Construcción completa antes de su siguiente ventana integral, sin pedir pruebas
manuales al propietario ni ampliar a servicios/regex/dependencias no autorizados.

El [contrato Explorer](M2B_EXPLORER_CONTRACT.md) ya inventaría la ruta TUI síncrona
y define catálogo único, identidades, cuotas y migración de ambos frontends. Es
diseño previo; el cierre de implementación se registra a continuación.

## Explorer TUI/GUI compartido — 2026-11-03

[Implementado](M2B_SHARED_EXPLORER.md): catálogo único por sesión con IDs opacos,
worker lazy, cuotas globales, cancelación/generaciones; vista compartida y apertura
confinada con dirty/pin/side/undo. Se retiró la enumeración y apertura alternativa
TUI. GUI tiene sidebar persistente, mouse/teclado, menú/palette, foco y modo compacto
que conserva split. Windows enumera por handle con ancestros fijados; fallback no
Windows mantiene ventana de namespace explícita, sin aceptación local.

Bloque completo construido antes de pruebas:352 casos Rust relacionados aprobados
(13 nuevos; un test del proveedor eliminado retirado),70 checks GUI y seis flujos
ConPTY reales. fmt/Clippy/release aprobados; fuentes y metadataGit propias intactas.
Evidencia cruda y hashes en M2B_SHARED_EXPLORER. Sin dependencias nuevas/CI/push.
M2A/M2B y gates globales/manuales permanecen abiertos. Continuación: completar la
paridad funcional GUI pendiente y preparar el API opcional de proveedores de
lenguaje según EXTENSION_ARCHITECTURE, sin asumir parser/LSP/ABL ya disponibles.

## Paridad de navegación GUI — 2026-11-03

[Bloque integrado](M2B_NATIVE_NAVIGATION.md): Ctrl+G/línea-columna escalar con campo
acotado, aceptación explícita y TaskStamp+ViewId/pane; atrás/adelante por grupo y
historial de consultas/options en Find/Replace. Menú/atajos/paleta comparten registro
y Workbench. Parser de posición compartido con TUI; cancelación de F3 diferido sin
parar el scan, borrador de Replace conservado a través de la paleta. Pin mantiene
su regla existente: Go to se dirige al otro grupo; back/forward usa el activo.

78 casos dirigidos aprobados (cuatro TUI nuevos),14 checks nativos de sección
navigation (siete nuevos), fmt/Clippy/release aprobados. El único fallo inicial fue
el oráculo del pin, corregido sin cambiar el modelo. No se repitieron suites
Explorer, recorrido nativo global ni perfiles cerrados. Evidencia cruda y hashes
retenidos; sin dependencias/CI/push ni pruebas manuales solicitadas.

Siguiente paridad GUI pendiente: edición universal de líneas y comentarios ya
disponible en jc-app/perfiles (duplicar/eliminar/mover y delimitadores del perfil),
con foco y registro compartidos antes de proveedores especializados. No ofrecer
handlers sin cablear. M2A/M2B y las puertas físicas/globales siguen abiertos.

## Paridad GUI de líneas y comentarios — 2026-11-03

[Bloque implementado](M2B_NATIVE_LINE_EDIT.md): seis comandos universales ya
cableados a teclado/menú Edit/paleta, Workbench y perfiles existentes. Disponibilidad
central requiere editor_input/foco/selección lineal/capacidad del lenguaje; captura
de origen de paleta rechaza documentos, revisiones, vistas y selecciones cambiados.
Sin segunda implementación de edición ni cambios al motor o semántica TUI.

Construcción amplia primero;23 casos dirigidos y14 checks nativos de sección
line-edit aprobaron a la primera (3/7 nuevos), fmt/Clippy/release aprobados.
Cuatro encodings, BOM/EOL, undo/redo, split/pin y modified limpio→sucio→limpio sin
Save observados. No repetir navegación/Explorer ni perfiles cerrados para docs.
Sigue pendiente la paridad de comandos universales restantes del registro en GUI
(p. ej. búsqueda sobre selección y reapertura de pestaña cerrada); luego API de
proveedores opcionales conforme al plan. No parser/AVM/CI nuevo; gates globales abiertos.

## Selección literal y ciclo de pestañas GUI — 2026-11-03

[Implementado](M2B_SELECTION_REOPEN.md): selección next/previous, reapertura,
cancelación explícita y posiciones de pestaña por menú/atajo/paleta sobre comandos
existentes. Source-selection exige foco/selección lineal y target actual; errores
de decode/cuota no sustituyen la consulta. Reopen propaga IO y conserva historial;
rechaza ruta ya abierta para impedir otro buffer. Cierre dirty confirma el mismo
doc/rev/vista/intención tras modal; solo la última vista crea metadata de cierre.

11 casos dirigidos (cuatro nuevos) y15 checks HWND de sección (ocho nuevos)
aprobados; fmt/Clippy/release aprobados. Fallos iniciales de montaje diferido/getter
privado retenidos, sin repetir otros casos aprobados ni suites cerradas.
Seguimiento M2B: capacidades universales GUI restantes de selección/rectángulo y
presentación según contratos ya existentes, antes de proveedores opcionales.
M2A/M2B y gates físicos/globales abiertos; no AVM/dependencias/CI/push nuevos.

## Selección rectangular GUI y presentación — 2026-11-03

[Bloque integrado](M2B_RECTANGLE_GUI.md): seis comandos/menú/paleta/AltShiftflechas,
Alt+drag con captura versionada, Source geometry absoluta compartida, clipboard
interno/edición/undo y cuadrícula TAB4 también con invisibles en GUI/TUI. No motor
rectangular nuevo ni ampliación de repertorio/cuotas. Se retiró la ruta visual
rectangular de escapes que confundía columnas; al cancelar retorna diagnóstico.

26 casos dirigidos (cuatro nuevos) y15 checks HWND (ocho nuevos), fmt/Clippy/release
aprobados. Dos bugs corregidos con evidencia: permiso TUI tras Find y salida de
rectángulo en BOF/EOF sin byte adyacente. Fallos y hashes retenidos, Source13B intacto.
No suites cerradas/perfiles/CI repetidos. Queda inventario final de paridad universal
GUI y puertas M2 físicas/globales; proveedores opcionales según arquitectura después.
Sin nuevas dependencias/instalación/AVM/push ni claim de columna Unicode general.

## Inventario universal GUI y edición/grupos — 2026-11-03

[Inventario completo](M2B_GUI_INVENTORY.md) y matriz con104 CommandId, entrada,
predicado/foco, ruta real y evidencia histórica acotada.85 con rutaGUI,19 sin
supported por dependencia/capacidad pendiente o adaptación Menu* de Win32.
No equivalen a104 funciones aceptadas ni a todos sus caminos probados.

Huecos independientes encontrados y cerrados juntos: Indent/Outdent ahora en
menúEdit; Link/UnlinkScroll en View/tecla/paleta e indicador persistente. Cuatro
ediciones (Indent/Outdent/Join/Split) exigen editor_input, también enTUI. Modelo
Workbench de bytes/vistas/undo/delta scroll conservado, sin proveedor nuevo.
Dos casosRust dirigidos (uno nuevo),12 checksHWND (cinco nuevos),fmt/Clippy/release
aprobados tras integrar todo; fallo inicialClippy conservado, sin suites cerradas.

Próximo bloque universal concreto: hostGUI de panel inferior, Problems/Output
F3 y foco de tres regiones. Concretar contrato de filas/destinos/cancelación y
estado antes de construir; esto no autoriza inventar proveedores. SearchResults
tiene panel de proyecto específico, no ese host. MenúsTools/Help, entrada avanzada
en selectores, selección lineal por drag/wheel y resto físico/global siguen
identificados, sin afirmar paridad completa. No CI/push/dependencias/AVM nuevos.

## Host GUI de paneles F3 y foco — 2026-11-03

[Bloque integrado](M2B_BOTTOM_PANELS.md): Problems/Output/ToggleBottomPanel en
View/paleta/teclado, selección por IDs, navegación validada y foco en tres regiones.
TaskPanels reemplaza logs de strings y conversión duplicada TUI, conservando una
sola copia de texto por fila y procedencia tipada; GUI acepta F3 en un único poll.
JobSystem captura ruta para invalidar SaveAs aun con revisión/encoding iguales.
Preservados límites F3, cuotas de log, fuente/vistas/undo, pin y render readonly.
No proveedor nuevo: productores sintéticos solo en automatización; vacío explícito.

23 casos dirigidos distintos (cuatro nuevos),13 checks HWND (seis nuevos),
fmt/Clippy/release aprobados tras cableado completo. Dos corridas nativas de la
misma sección: final incluye procedencia visible; no suites antiguas sin impacto,
mediciones globales o CI. SaveAs de memdoc en destino propio nuevo prueba identidad;
archivo exacto verificado/eliminado, fuente13B y metadata Git sintética intactos.

Ahora88 de104 comandos tienen ruta GUI; no aceptación de todos sus caminos.
Pendientes universales concretos del inventario: entrada avanzada/pegado de
selectores, adaptación incompleta de menús y selección lineal por drag/wheel.
Proveedores internos/terminal/regex requieren sus propios contratos/puertas; no
inferir implementaciones de negocio del host. Manual/global/Unicode/clipboard OS,
symlink y notices GNU siguen abiertos. Sin dependencias/proveedores/AVM/CI/push nuevos.

## Entrada avanzada de selectores GUI — 2026-11-03

[Bloque integrado](M2B_SELECTOR_INPUT.md) en Quick Open/paleta/encoding: campo único
con caret/anchor, Shift selección, Home/End/Delete, Ctrl+A/C/X/V/U y rutas de menú
Edit, sin confundir edición de campo con comandos del invocador de paleta. Up/Down/
Page/Tab navegan candidatos; CtrlHome/End primero/último. Cuota256UTF8 y rechazo
atómico de controles/bidi/overflow; handoff externo fuera de préstamos, revalidado
por generación/intención/candidato/doc/vista/encoding/ruta/raíz/foco. Solo transporte
sintético en pruebas; no clipboard privado leído/alterado durante desarrollo.

FieldSelection reutiliza edit_field y un walker de presentación común; retiene
solo filas visibles del campo (dos pasadas <=4096B), sin otra historia/motor fuente.
23 casos Rust distintos aprobados (24 pases con un solapamiento de filtro), cuatro
nuevos;13 checks HWND (seis nuevos), fmt/Clippy/release. Fallos de expectativa/
getter privado/enum grande retenidos y corregidos; native aprobó primera corrida.

Continúan88 rutasGUI, sin reclamar104 funciones aceptadas. Pendientes universales
del inventario: adaptación de menús nativos y selección lineal por drag/wheel.
Proveedores/terminal/regex, aceptación física/global/Unicode/clipboardOS, symlink
y notices GNU conservan sus puertas. Sin nuevas dependencias/AVM/CI/push/publicación.

## Menús nativos y gestos de fuente — 2026-11-03

[Bloque integrado](M2B_NATIVE_INTERACTION.md) posterior a `ae9a813`: menús desde
el modelo compartido y handlers reales, sin rangos numéricos. Seis macros de
paleta y Alt/F10 abren HMENU; elección revalidada por contexto/capability/origen,
atajos/checks según control activo. Sin contenido inventado para Build/Help.
Ahora94 rutasGUI de104 IDs históricos:88 acciones y seis macros, sin certificar
todos sus caminos físicos ni alterar el inventario histórico.

Drag lineal/Shift mantiene ancla/grupo, convierte proyección a bytes legales y
rechaza destinos obsoletos. Captura común con rectángulo fuera del préstamo.
Wheel fraccionaria signed sobre el grupo bajo el puntero conserva foco/caret/
selección; viewport manual por vista y enlace por delta, pin independiente.
Se eliminan la antigua ruta click mutable y el constructor de menús numérico.

Construcción completa antes de pruebas:16 Rust distintos (cuatro nuevos),13 HWND
(seis nuevos), fmt/Clippy/release aprobados. Fallo inicial de tres lints de arnés
retenido, native a la primera; ZIP local verificado. Sin suites cerradas repetidas
sin impacto, perfiles globales o CI. Estos dos huecos quedan cerrados localmente;
pantalla/ratón/teclado físicos, clipboard OS/IME/DPI/accesibilidad, Unicode general,
latencia/RAM global, mouse horizontal/autoscroll/word-selection/campos y symlink/
notices siguen pendientes. Proveedores/terminal/regex conservan contratos y puertas
propias. No inferir M3 ni distribución certificada. Continuar en bloques amplios
y dejar pruebas manuales/OS con el usuario al final, según su preferencia.

## Seguimiento de validación y límite de admisión — 2026-11-03

[Auditoría previa y evidencia](M2B_MENU_INVOKER_VALIDATION.md), base171af05.
Se cierra el recorrido comprometido paleta→macro de menú con Find/Replace:
seis macros, tres estados de campo, cancelación repetida, continuidad del teclado,
acción View permitida, edición fuente rechazada y ocho cambios tardíos de draft.
Un check integrado nuevo + siete de arranque/pintura, tres rechazos CLI y fmt/
ClippyGUI/releaseGUI aprobados a la primera. Sin pruebas unitarias, repetición de
la sección histórica13 o cambios de handlers; no nueva funcionalidad de producto.

La auditoría de la mención previa a mouse horizontal/autoscroll/word-selection/
campos no la convierte en admisión: son limitaciones/exclusiones, sin contrato
específico de siguiente bloque. Su construcción no se activa. Hace falta decidir
alcance, coordenadas, límites y cancelación del incremento antes de implementarlo.
Esta nota registra la decisión de no ampliar, sin añadir funcionalidades al plan
para justificar código retrospectivamente. M3/proveedores y puertas físicas/globales
no se infieren ni se cierran; sin dependencias/AVM/CI/push/publicación nuevos.

## Cierre local — resaltado compartido, 2026-11-03

Por prioridad explícita de Juan se integró resaltado léxico ABL, Rust, JSON y
JSONC en TUI y GUI; otros perfiles permanecen Plain. Contrato, límites, fallos
iniciales y evidencia: [M2_SYNTAX_HIGHLIGHTING.md](M2_SYNTAX_HIGHLIGHTING.md).
Pasaron35 casos Rust distintos (18 nuevos),11 checks HWND (4 nuevos),5 flujos
ConPTY,3 rechazos CLI, fmt, Clippy y build release locales offline. No se ejecutó
toda la suite runtime del workspace ni CI. No equivale a aceptación física/global.

El diff entre documentos abiertos queda suspendido y recuperable en
`checkpoints/open-diff-before-syntax-66aa783.zip` y su manifiesto; no está activo
ni validado. No restaurarlo automáticamente sobre el resaltado.

La autorización posterior del usuario permite incorporar y fijar una dependencia
Rust mantenida para regex real (candidato regex-automata) en el siguiente bloque
Search Plus. Sustituye la restricción anterior para esta dependencia concreta:
no pedir de nuevo permiso. No se añadió en este bloque, regex sigue deshabilitado
y no están autorizadas instalaciones de programas del sistema.

## Regex implementado — continuación TUI, 2026-11-03

Regex real de documento y proyecto está integrado con regex-automata=0.4.18,
autorizado por Juan. Alt+R, historial, rangos originales, cancelación/cuotas y
reemplazo literal por transacciones; [contrato y evidencia](M2_REGEX_SEARCH.md).
Esta actualización sustituye las menciones históricas de regex deshabilitado.
124 casos distintos aprobados observados, cuatro flujos ConPTY y fmt/Clippy/
release locales. Dos fixtures junction y seis Git fallaron por creación/acceso
restringido: no se declara suite global aprobada ni aceptación física/RAM.

La instrucción vigente de Juan es continuar hasta terminar los pendientes TUI,
en bloques amplios, validar al cerrar cada bloque y activar el siguiente sin
esperar otro «continúa». Siguiente bloque: comparación de documentos abiertos
sin Git, desde el checkpoint revisado sobre el código actual. No restaurar fuentes
antiguas sobre resaltado/regex. Proveedores ABL/AVM y aceptación física conservan
sus requisitos reales; no afirmar capacidades que solo tengan contrato.

## Comparación de abiertos TUI implementada — 2026-11-03

El checkpoint suspendido de comparación sin Git ya se integró sobre regex y
resaltado actuales: controller con dos orígenes versionados, admisión dual F3,
presentación compartida acotada y visor TUI lateral/unificado. Comandos View/
paleta, hunks, navegación a grupo original, stale/refresh/cancel; fuente de solo
lectura y sin writes. [Contrato y evidencia](M2_OPEN_DOCUMENT_DIFF.md).
GUI de este visor sigue pendiente; no restaurar su checkpoint automáticamente.

54 casos distintos observados aprobados, cuatro flujos ConPTY, fmt/Clippy/release.
Las nueve regresiones Git pasaron con acceso autorizado a fixtures propios tras
fallar por acceso sandbox; no se usa Git en la comparación del producto.
No CI ni aceptación física/global. Siguiente bloque realmente activo: contrato
de capacidades internas opcionales y consumidor TUI de símbolos/navegación,
según EXTENSION_ARCHITECTURE y [matriz de cierre TUI](M2_TUI_FINISH.md).

## Capacidades locales de fuente TUI — 2026-11-03

Primer registro interno namespaced y proveedor opcional progress.abl operativo
para símbolos y referencias léxicas; core.words completa palabras textuales de
cualquier documento. Listas/selector TUI, entrega F3, stale, candidatos confinados
y propuestas de completado por revisión/una transacción. [Contrato y evidencia](M2_SOURCE_SERVICES.md).
40 casos distintos (17 nuevos), cuatro flujos ConPTY, fmt/Clippy/release locales
aprobados. No AVM, semántica, loader externo, GUI ni aceptación física/global.

Siguiente bloque activo: plantillas/snippets universales y catálogo ABL del
proveedor opcional, parámetros/preview/aceptación y undo único. Continuar sin
esperar otro «continúa»; construir el conjunto antes de su ventana de validación.

## Snippets TUI — cierre local 2026-11-03

Motor universal acotado con parámetros, preview y aceptación por revisión/una
transacción. progress.abl aporta siete plantillas declarativas de Insert/Surround;
sin reglas ABL en jc-app. Preserva encoding/BOM/EOL/undo; propuestas obsoletas se
rechazan. [Uso y evidencia](M2_SNIPPETS.md):38 casos distintos (11 nuevos), cuatro
recorridos ConPTY, fmt/Clippy/release aprobados. Sin AVM ni aceptación física/global.
Sigue auditoría final de teclado, foco y flujos TUI; CI permanece en M5.

## Auditoría de overlays/teclado TUI — 2026-11-03

Pegado y teclas respetan el overlay superior sobre snippets/completado/diff.
Rutas/prompts4096 UTF8, posición64 y paleta256; rechazo atómico al exceder cuota.
CtrlU vacía campos; Home/End/Page navegan menú/paleta sin perder selección visible,
Page en preview de reemplazo usa filas de la última presentación. Sin segundo buffer.

117 casos TUI distintos aprobados (cinco nuevos), entre corrida y cierre dirigido.
Dos expectativas históricas se actualizaron a regex habilitado y al nuevo final
del menú View; fixture de menú Help vacío corregido. Fmt/Clippy/release aprobaron.
Tres flujos ConPTY finales aprobaron,27557 bytes VT/salida0/fixture intacto.
Tres intentos iniciales quedaron esperando el título de la paleta, confundido con
una entrada del menú por el borde del snippet; se corrigió ese oráculo. El primer
intento nunca llegó a enviar bracketed paste: no acredita ni refuta ese transporte.
Event::Paste y sus cuotas sí están cubiertos por casos Rust; no clipboard OS real.

Siguiente bloque activo: adaptar clipboard Windows estricto a la TUI, solo por
acción explícita; separar validación inyectada de interoperabilidad física.

## Clipboard Windows TUI — cierre local 2026-11-03

Adaptador CF_UNICODETEXT de la GUI extraído a jc-platform (sin dependencias
externas); ambos frontends comparten implementación. TUI conecta transporte en
el loop real y crea HWND propio invisible bajo demanda, solo por Copy/Cut/Paste.
Headless conserva clipboard interno; pruebas inyectan memoria, no el clipboard
personal. Cortar después de publicar, pegar por destino/revisión y un undo;
cuotas/encoding estricto, campos del overlay superior y rectángulos internos.

32 casos Rust distintos aprobaron: cinco de plataforma (cuatro migrados y
uno nuevo de HWND propio), siete TUI nuevos y regresiones de clipboard/F1/registro.
Fmt, Clippy workspace/all-targets y release bins/examples aprobaron offline.
GUI HWND propio:13 checks selector-input, GDI sin error, fixture y metadata Git
intactos, clipboard sintético. ConPTY TUI: tres flujos,27436 bytes VT/salida0,
fixture intacto. No lectura/escritura de clipboard personal ni certificación OS.
Un fixture usó un nombre de método inexistente y un campo tenía visibilidad más
amplia que su trait; se corrigieron antes del cierre, logs retenidos.

[Contrato y límites](M2_TUI_CLIPBOARD.md). Sigue layout Unicode general: Juan
autorizó unicode-segmentation=1.13.3 y unicode-width=0.2.2; [admisión](M2_UNICODE_LAYOUT_ADMISSION.md).
Esa autorización sustituye la restricción previa para estas dos dependencias;
no pedirla de nuevo. Sin instalación de programas, CI ni AVM.
## Enlaces, esquema y renombrado

La navegación lateral de enlaces Markdown y el Esquema ABL de la GUI están
integrados. El proveedor ABL incorpora renombrado de variables de documento
y locales de procedimiento o función en un subconjunto comprobable, con
vista previa y una transacción. También admite declaraciones y llamadas
estáticas a procedimientos y funciones internas en el mismo archivo, incluidos
los pares de prototipo `FORWARD` e implementación con firmas compatibles.
Las inclusiones ABL encadenadas y las llamadas indirectas comprobables
ya cuentan con renombrado de proyecto.
Ver [alcance y orden de implementación](ENLACES_ESQUEMA_RENOMBRADO.md).

La [navegación por proveedor](NAVEGACION_MULTILENGUAJE.md) separa llamadas ABL
de inclusiones y conecta el Esquema y las dependencias de JavaScript.
**Llamado por** incorpora navegación inversa directa desde fuentes en disco
para ambos proveedores, con apertura lateral y revalidación antes del salto.

## Expansión DataDigger

La cuadrícula consulta registros ABL, permite editar celdas de texto y entero
en copias locales autorizadas y restaura el último cambio confirmado. La
edición y restauración requieren vista previa y respaldo verificado antes de
escribir. El recorrido GUI/TUI y los conflictos se validaron con Sports2000
en OpenEdge 11.7. Altas y borrados tienen vista previa y reversión por clave.
La escritura directa en bases activas locales de OpenEdge 11.7 admite los
disparadores configurados del proyecto. El respaldo JSON registra únicamente
la fila seleccionada para cada operación solicitada en DataDigger, y el
historial se consulta a petición. La restauración revalida la imagen de la
fila antes de escribir. El proveedor compartido de PostgreSQL y MySQL
consultó catálogo y registros mediante TLS con ambos controladores ODBC
Windows. Queda verificar OpenEdge 10.2B y, al final, 10.0B.
[Plan de expansión](EXPANSION_DATADIGGER.md).

# Distribución flexible y comparación paralela Git

Bloque de GUI nativa — 2026-11-06. Extiende el [espacio Git](M2_GIT_WORKSPACE_NATIVE.md)
y los [pares locales](G1_LOCAL_SOURCE_PAIRS.md). El tema #2E3440 y la tipografía
siguen siendo los de CIDE; el núcleo Git y los documentos no cambian.

## Distribución y navegación

El control Distribución ofrece una columna, dos con archivos a la izquierda,
dos con archivos a la derecha y tres columnas. Las áreas son Historial, Archivos
y Comparación. Los divisores se arrastran; el tamaño mínimo escala con DPI.
Si la ventana no admite la distribución solicitada, se reduce a dos o una columna
sin perder la preferencia: al ampliar la ventana reaparece la elegida.
Las pestañas de área y Tab en modo compacto mantienen accesibles las tres áreas.
Elegir commit conduce a Archivos; elegir un archivo conduce a Comparación.
El desplegable usa el tema y admite flechas, Intro y Esc. Un clic fuera lo cierra
sin ejecutar el control que hubiera debajo. Sus objetivos se publican al pintar
y caducan al cambiar la geometría, las métricas tipográficas o la versión visible.

La vista V alterna Unificada/Paralela; también hay un botón explícito. En paralelo,
ambos lados comparten filas de contexto y desplazamiento vertical. Los reemplazos
se alinean por posición de línea dentro del bloque y las diferencias de longitud
usan celdas vacías; no se afirma emparejamiento semántico ni diff por carácter.
Se mantiene el modelo de bloques con tres líneas de contexto y Anterior/Siguiente.
No es aún una vista continua de todos los bloques del archivo.

La comparación dispone de barras vertical y horizontal cuando hay desbordamiento,
arrastre de sus controles, rueda vertical, Mayús+rueda y flechas izquierda/derecha
para horizontal. El máximo horizontal se calcula sobre las filas visibles,
no mediante un recorrido del documento completo. Al cambiar a líneas cortas se
normaliza la posición y se reproyecta como máximo una vez. El arrastre usa captura
nativa propiedad del cliente y termina al soltar, perder captura o invalidarse
el cuadro, sin modificar cursor, selección, bytes ni undo.

## Un único proyector de filas

`git_compare_rows.rs` sustituye la duplicación anterior entre el buffer y las
revisiones. Buffer, histórico y pares locales usan el mismo proyector prestado.
El modelo de comparación permanece en jc-git; este módulo solo crea celdas GUI.
Por página admite 128 filas lógicas, hasta 256 celdas en paralelo, 64 KiB de
fuente por celda y 4096 bytes/512 columnas de texto visible; el desplazamiento
visual se limita a 16384 columnas. TAB4, escapes de bytes inválidos y límites
de grafemas se conservan. Una fila que supera la cuota muestra [fila limitada]
al principio. Se descarta el último grafema potencialmente incompleto del prefijo
acotado, conservando intacta la fuente original.
No se crea otro Document, modelo de edición ni copia del historial.
La preparación normaliza exclusivamente metadatos de viewport; no acepta entregas
ni abre repositorios, directorios o archivos. El cambio de distribución/vista
reutiliza la comparación ya aceptada.

## Preferencias recuperables

Los metadatos GUI v5 añaden siete bytes versionados con distribución, dos ratios
de divisores y modo paralelo. Se validan antes de adoptar el Workbench restaurado.
El lector conserva v1/v2/v3/v4 y sus valores por defecto; los documentos, APP2,
generaciones, HEAD/BACKUP y retención no cambian. Cambiar proyecto conserva estas
preferencias. La recuperación periódica/final existente las publica junto con
los demás metadatos. Con recuperación deshabilitada no se promete persistencia.
Las versiones anteriores de la GUI no interpretan metadatos v5; usar los nuevos
binarios para las sesiones escritas con ellos.

## Validación local

107 casos de GUI/entrada aprobaron (101+6), quince nuevos. Tras ajustar el recorte
Unicode y reutilizar la anchura de página calculada por el proyector, el cierre
Git dirigido aprobó 26 casos (25+1). No se repitió el workspace runtime completo
ni se presenta el total histórico de pruebas del core como una ejecución nueva.
Se conservan dos fallos de compilación de integración corregidos antes del cierre:
el préstamo del vector de checks y la vida del iterador de grafemas.
Formato y Clippy workspace/todos los destinos aprobaron offline. Release workspace
y reconstrucción final GUI usan target/g2-layout-next/release; se preservan las
rutas anteriores. No se añadió ninguna dependencia ni se cambió Cargo.lock.

## Alcance restante

Este bloque cumple distribución flexible, divisores, preferencias y vista paralela.
La expansión G2 aún puede añadir árbol Cambios/Archivos, pestañas por archivo y
menús contextuales; G3 conserva historial por ruta, anotaciones y exportación de
parches. No se declaran terminados G2/G3 completos. Escrituras y red Git siguen
reservadas a M5. La TUI conserva su composición de consulta existente; este bloque
no añade un layout gráfico a la terminal.
La revisión física M2 sigue condicionada al escritorio disponible; IME excluido,
Narrador aplazado y traslado entre monitores sin verificar. No hay CI ni push.

147 comprobaciones HWND aprobaron, incluidas tres nuevas que ejercitan las cuatro
distribuciones mediante clics publicados, la proyección paralela y la captura
real de ratón para ambas barras y el primer divisor. La comparación larga usa
160 líneas anchas y se deshace antes de continuar las pruebas de edición previas.
PATH del proceso GUI vacío; metadatos Git y archivo original intactos.
El primer escenario requirió enviar WM_PAINT al HWND oculto: UpdateWindow no
lo pintaba. Otro intento comparaba contra el estado anterior a la primera pintura
del fixture largo inyectado; ahora se estabiliza esa pintura antes del snapshot.
Después, todas las acciones conservaron exactamente el estado de documento/vistas.
Los fallos iniciales quedan en g2-layout-native*.log, sin presentarlos como aprobados.

No se repitieron pruebas físicas de escritorio ni ConPTY. El pendiente anterior
de revisión visible y salida normal debe renovarse mediante Computer Use antes
de actuar sobre una ventana; no se fuerza su terminación desde este bloque.

El cierre optimizado también aprobó 147 comprobaciones, después de generar las
capturas de una, dos (izquierda/derecha) y tres columnas. Las capturas se revisaron
sin solapamientos de controles. Informe final: work/g2-layout-native-summary.json.
Evidencia resumida: [JSON](evidence/g2-git-layout-comparison-2026-11-06.json).

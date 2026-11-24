# Esquema completo y relaciones por niveles

El bloque elimina el recorte de 63 declaraciones ABL y los topes de 64/512
resultados de los proveedores internos en las consultas de Esquema. La GUI
conserva todos los elementos aceptados y solo dibuja las filas visibles; la TUI
también conserva todos los resultados y permite llegar al último. La búsqueda
de contenedores del árbol y la deduplicación ABL evitan recorrer todas las
declaraciones anteriores por cada variable.

«Llama a» proyecta tres niveles de relaciones estáticas, con padres tipados,
destinos fijados a su contenido y detección de ciclos por rama. Las rutas de
archivo son filas de relaciones, sin introducir niveles de subcarpetas. Los
destinos externos abren su archivo y las rutinas locales su declaración; un
cambio del destino invalida su apertura. Los destinos dinámicos, ambiguos o que
el proveedor léxico no resuelve permanecen explícitos como no resueltos. Esto
no equivale a un grafo semántico del compilador o del programa en ejecución.
El origen activo usa su captura del editor, incluidos cambios sin guardar;
los archivos externos del recorrido se analizan desde disco, sin guardarlos
ni sustituir los buffers abiertos del usuario.

«Llamado por» conserva un nivel de llamadas directas. El recorrido inverso de
archivos estáticos del proyecto termina el catálogo, en lugar de detenerse a
los cuatro segundos, 4.096 archivos o 128 MiB acumulados. Usa hasta dos
trabajadores según la CPU disponible. Cada trabajador conserva como máximo una
entrega adelantada; el coordinador publica por orden de archivo. No hay un
pool por archivo. El recorrido mantiene las comprobaciones de handles, raíz y
reparse del explorador, usando una entrada de enumeración de fondo que no
recorta un directorio a 2.048 nombres. Se excluyen .git, .cide, target y
node_modules del alcance del proyecto.

Las consultas comparten JobSystem y sus generaciones, cancelación y revisión
de fuente, vista, ruta y codificación. El progreso pertenece a la generación
admitida: los workers solo actualizan contadores y notifican, sin tocar la GUI
ni los buffers. La GUI dibuja una barra con porcentaje y la TUI una barra
textual. La entrega de resultados sigue ocurriendo únicamente al recoger las
tareas; editar o cambiar el contexto rechaza entregas obsoletas.

## Finalización y recursos

No se presenta una lista de primeras N entradas como un análisis completo.
Los errores de archivos/directorios producen el estado tipado Incompleto y un
aviso; las entradas válidas siguen siendo navegables. El agotamiento de recursos,
cancelación o fallo completo tiene su propio estado y no publica una lista
silenciosamente truncada. Las inclusiones ausentes también se notifican.

Esto no elimina los límites de seguridad/memoria ni las limitaciones de los
analizadores léxicos. Las consultas internas completas admiten hasta 64 MiB de
fuente y 64 MiB de resultados por trabajo, 128 MiB de capturas conjuntas y
128 MiB de caché de fuentes de la expansión de llamadas. La enumeración de
un directorio tiene 64 MiB de metadatos; el timeout cooperativo es de 30 minutos.
Estos límites fallan explícitamente. Son presupuestos de carga útil, no una
cota de memoria privada total de índices, árboles y buffers temporales.
Se respetan las cuotas personalizadas de JobSystem y los proveedores de
prueba/ajenos conservan su admisión anterior.
Las consultas de edición, completado, LSP y Git mantienen sus contratos previos.
Las inclusiones ABL con ubicaciones ya no se recortan a 32 archivos; ciclos,
lecturas imposibles y el límite protector de profundidad se señalan.

## Validación local Windows

Rust 1.88.0, offline, caché target/embedded-git-next y dos jobs de compilación.
Evidencias y fallos iniciales retenidos bajo work/source-complete.

- jc-app, integración source_services: 18 casos, con 2.100 archivos en un mismo
  directorio, fuente de más de 2 MiB, generaciones obsoletas, ciclos, tres niveles
  en Rust/C#/Java/JS/TS/PHP y navegación al destino.
- jc-progress, source_services con la feature source-services: 12 casos,
  incluida expansión ABL entre archivos, apertura de un destino de 3 MiB,
  rechazo de destinos cambiados y el estado
  Incompleto cuando un archivo supera el presupuesto de lectura.
- GUI, filtro outline_: 21 casos aprobados y un caso OpenEdge real excluido.
  Incluye 1.500 variables, navegación a la última, 40 inclusiones y una ventana
  nativa propia que verifica por píxel la barra de progreso durante una consulta
  real y abre includes/relaciones mediante los mensajes de entrada de la GUI.
- TUI, source_services_tests: 17 casos aprobados y nueve de entorno real
  excluidos; incluye las 1.500 entradas, navegación con Fin/Intro al último destino
  y jerarquía de tres niveles.
- JobSystem: ocho casos de cuotas, cancelación, deadlines, pánico y obsolescencia.
- jc-files scoped_directory: tres casos en contexto normal de Windows. El
  entorno restringido bloqueó la creación del junction del fixture; esa primera
  corrida se conserva y no se declara aprobada.

Se ejecutan Clippy con todos los targets afectados y compilación GUI/TUI debug.
No hay nueva certificación de Linux, ejecución AVM, latencia física, ni interacción
con el escritorio personal. No se genera MSI, push ni GitHub Actions en este bloque.

Entrega local completa: target/source-complete-next/debug, con cide-gui.exe,
cide-tui.exe, manifiesto de hashes y runtime OmniCore verificado. Las entregas
anteriores y la caché de compilación se conservan.

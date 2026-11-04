# Enlaces, esquema y renombrado

En un archivo Markdown, **F8** abre la lista de referencias y selecciona el enlace bajo el cursor. **Intro** abre el destino en el grupo actual y **Ctrl+Intro** lo abre al lado. Está disponible en la GUI y la TUI. La vista previa de Markdown se mantiene como una vista de lectura.

Se reconocen enlaces en línea como `[Guía](docs/guia.md)`, con destinos relativos al documento y espacios codificados como `%20`. La apertura queda confinada a la raíz del proyecto o, sin proyecto, a la carpeta del archivo abierto. Se abre el archivo de un enlace con fragmento, sin saltar al encabezado. Los destinos web, anclas sin archivo, enlaces por referencia y rutas absolutas quedan fuera de este bloque.

En la GUI, **Esquema** aparece debajo del Explorador cuando el documento tiene un proveedor de símbolos y hay espacio. Se puede enfocar con **F6** y navegar con las flechas o la rueda. **Intro** y doble clic saltan a la declaración. El proveedor ABL lista procedimientos, funciones, clases, métodos, variables, parámetros, búferes, tablas temporales e inclusiones estáticas, entre otras declaraciones. La lista sigue el orden del archivo y conserva el presupuesto del proveedor de 2 MiB de fuente y 64 resultados. El esquema se actualiza en segundo plano y descarta resultados de otra revisión.

## Bloques de renombrado pendientes

1. Resolver identidades de declaraciones ABL y sus usos. El primer alcance cubre variables declaradas dentro de un procedimiento, función o método. El proveedor debe resolver la variable a partir de la declaración o de un uso, respetar los nombres sombreados y rechazar resultados ambiguos. Añadir después variables de nivel de documento y miembros de clase con sus reglas de visibilidad.
2. Entregar una vista previa de rangos tipados y verificar identidad del documento, revisión y codificación antes de aplicar. Reutilizar la transacción existente para obtener un paso de deshacer. Cubrir en pruebas nombres repetidos en ámbitos distintos, comentarios y cadenas, calificadores y referencias a campos, macros e includes sin resolución y colisiones con el nombre nuevo.
3. Extender el proveedor a procedimientos, funciones y clases con resolución entre archivos. Distinguir llamadas locales, referencias dependientes de PROPATH, miembros calificados y llamadas dinámicas. Antes de habilitar el cambio de proyecto, definir qué archivos quedaron sin analizar, verificar todos los orígenes de la vista previa y definir la recuperación ante aplicación parcial.

El listado léxico de símbolos permite navegar. El renombrado requiere resolver cada uso a su declaración antes de proponer cambios.

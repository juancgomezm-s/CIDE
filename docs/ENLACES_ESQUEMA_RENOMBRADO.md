# Enlaces, esquema y renombrado

En un archivo Markdown, **F8** abre la lista de referencias y selecciona el enlace bajo el cursor. **Intro** abre el destino en el grupo actual y **Ctrl+Intro** lo abre al lado. Está disponible en la GUI y la TUI. La vista previa de Markdown se mantiene como una vista de lectura.

Se reconocen enlaces en línea como `[Guía](docs/guia.md)`, con destinos relativos al documento y espacios codificados como `%20`. La apertura queda confinada a la raíz del proyecto o, sin proyecto, a la carpeta del archivo abierto. Se abre el archivo de un enlace con fragmento, sin saltar al encabezado. Los destinos web, anclas sin archivo, enlaces por referencia y rutas absolutas quedan fuera de este bloque.

En la GUI, **Esquema** aparece debajo del Explorador cuando el documento tiene un proveedor de símbolos y hay espacio. Se puede enfocar con **F6** y navegar con las flechas o la rueda. **Intro** y doble clic saltan a la declaración. El proveedor ABL lista procedimientos, funciones, clases, métodos, variables, parámetros, búferes, tablas temporales e inclusiones estáticas, entre otras declaraciones. La lista sigue el orden del archivo y conserva el presupuesto del proveedor de 2 MiB de fuente y 64 resultados. El esquema se actualiza en segundo plano y descarta resultados de otra revisión.

## Renombrado de variables ABL

En GUI y TUI, coloca el cursor dentro de una declaración `DEFINE VARIABLE`
o de uno de sus usos y pulsa **F2**. También puedes elegir **Renombrar variable**
en la paleta. Escribe el nombre nuevo y pulsa **Intro** para ver la lista de
cambios con su línea. **F9** o **Ctrl+Intro** aplica la lista completa como
una sola transacción de deshacer. **Esc** cancela.

El proveedor Rust resuelve variables escalares de nivel de documento y locales
de procedimientos y funciones explícitos. Distingue declaraciones homónimas
por ámbito, reconoce parámetros como posibles sombreados y compara nombres
sin distinguir mayúsculas. Conserva comentarios, cadenas, codificación y
finales de línea. El análisis se ejecuta sobre una captura acotada y la
aplicación exige la misma revisión, vista, codificación y posición del cursor.

El primer subconjunto admite declaraciones escalares, asignaciones simples
y ciertos bloques y expresiones. Rechaza documentos con construcciones cuya
resolución no puede verificar, como includes, macros, acceso a tablas o campos,
clases y métodos. Rechaza colisiones y nombres que no cumplan el subconjunto de
identificadores admitido. Admite hasta 256 KiB de fuente y 64 cambios por
vista previa. La validación es local en Rust y queda pendiente la validación
con un compilador OpenEdge.

## Bloques de renombrado posteriores

1. Ampliar la resolución de variables a miembros de clases y métodos, accesos
   calificados, buffers, macros e includes conforme haya evidencia del contexto.
2. Extender el proveedor a procedimientos, funciones y clases con resolución
   entre archivos. Distinguir llamadas locales, referencias dependientes de
   PROPATH, miembros calificados y llamadas dinámicas. Antes de habilitar el
   cambio de proyecto, definir qué archivos quedaron sin analizar, verificar
   todos los orígenes de la vista previa y diseñar la recuperación ante aplicación parcial.

El listado léxico de símbolos permite navegar. El renombrado requiere resolver cada uso a su declaración antes de proponer cambios.

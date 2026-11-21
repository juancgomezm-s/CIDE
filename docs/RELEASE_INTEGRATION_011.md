# Integración desde CIDE 0.1.8

Revisión local del 20 de noviembre de 2026.

## Entregas publicadas

| Entrega | Revisión | Contenido |
| --- | --- | --- |
| 0.1.8 | 501602d | Base publicada de OmniCore, contexto y completado SQL. |
| 0.1.9 | 93db0ec | Correcciones del autocompletado de OmniCore y consulta SQL. |
| 0.1.10 | 7c7df90 | Vínculos RUN de ABL, historial, última edición y selector TUI. |

El MSI 0.1.10 se construyó desde una instantánea del índice de Git. Su
extracción administrativa verificó 1.057 archivos por tamaño y SHA-256.
Los cambios locales anteriores de Esquema, apariencia y recuperación del
catálogo de OmniCore quedaron fuera de esa entrega.

## Cambios locales en integración

- Esquema con navegación de inclusiones, raíz del repositorio anidado y
  búsqueda inversa que conserva resultados parciales.
- Recuperación del acceso al catálogo de ChatGPT en OmniCore.
- Preferencias con vista previa, tema para controles y fuentes de 11 y 12 px.
- Historial compartido accesible con Ctrl+Izquierda, Ctrl+Derecha y Ctrl+Q
  desde el editor y los paneles. El salto hacia un grupo fijado conserva
  el origen en el historial del destino.
- Contenido conserva declaraciones y dependencias. La pestaña Relaciones
  contiene los folders Llama a y Llamado por con resultados navegables.
- Acerca de CIDE en Herramientas para GUI y TUI. Muestra el titular
  del instalador, versión de producto, revisión y cambios locales.

La compilación de desarrollo se identifica como 0.1.11-dev. La versión de
producto de las próximas entregas se configura con CIDE_RELEASE_VERSION.
Las versiones internas de los crates siguen independientes de la del MSI.

## Cobertura y comprobación pendiente

El historial es independiente del lenguaje. La navegación RUN usa el
resolvedor ABL. Los vínculos de métodos usan el controlador compartido
FollowLink en Rust, C#, Java, PHP, JavaScript y TypeScript, con resolución
local de llamadas y referencias de función o callback. Ctrl+Intro,
Ctrl+clic y doble clic comparten la acción en GUI y TUI. Un destino externo
se consulta mediante definición LSP cuando existe un proyecto y el servidor
correspondiente está configurado. Las relaciones léxicas están disponibles para
ABL, Rust, C#, JavaScript, TypeScript, PHP y Java. La resolución semántica
entre archivos usa los servicios LSP configurados para cada lenguaje.
Una lista léxica vacía no certifica que no existan llamadas externas.

La primera revisión visible quedó bloqueada por GetCursorPos, acceso denegado.
Después de habilitar el escritorio se comprobó la GUI con Computer Use.
Se abrió Acerca de CIDE desde Herramientas y se revisó el rediseño con icono,
versión, autor y compilación. En el ejemplo Rust se expandieron ambos folders,
se siguió destino con doble clic, se abrió la sentencia llamadora en principal
y se recorrieron ambas posiciones con Ctrl+Izquierda y Ctrl+Derecha.
En ABL, Ctrl+Intro sobre RUN abrió destino.p y ambos atajos devolvieron
origen.p, línea 2 columna 12, y el archivo de destino.
Se insertó un espacio en el destino sintético, se volvió al origen y Ctrl+Q
regresó a la línea editada del destino. Ctrl+Z retiró la edición de prueba.

La suite final de la GUI pasó 210 pruebas con 15 omisiones explícitas.
La suite de la TUI pasó 198 con 14 omisiones antes del ajuste visual final
del cuadro y del agrupamiento GUI. Formato y Clippy de ambos frontends
pasaron después de estos ajustes.

Las pruebas locales de relaciones incluyen C#, Java, PHP, Rust, JavaScript
y TypeScript. Esa matriz automatizada se distingue de la comprobación
visible de Rust y ABL. Las pruebas que requieren perfiles reales de
OpenEdge o servidores LSP mantienen sus condiciones de ejecución.

## Ampliación de vínculos de métodos

La matriz compartida verificó destino y regreso para llamadas en los seis
lenguajes anteriores, más referencias de métodos sin invocación, callbacks
y la forma this::metodo de Java. Las pruebas de entrada de la TUI verificaron
Ctrl+Intro y Ctrl+clic para los seis lenguajes. La suite GUI pasó 210
pruebas y la TUI 199, con sus 15 y 14 omisiones de entorno respectivamente.
Clippy de ambos frontends pasó. Esta ampliación conserva los límites
de las capturas de los servicios de código.

Al intentar actualizar la ventana para el recorrido visible multilenguaje,
Windows volvió a devolver GetCursorPos, acceso denegado, 0x80070005,
también después de seleccionar de nuevo la ventana. La comprobación visible
anterior de los folders y los atajos sigue registrada arriba. El nuevo
gesto directo sobre métodos de los seis lenguajes necesita ese recorrido
con el escritorio disponible.

## Integración para 0.1.11 — 21 de noviembre

Se integran todos los cambios locales enumerados anteriormente en main.
La revisión de escritorio de este bloque comprobó, sobre links.rs,
el doble clic y Ctrl+Intro: ambos movieron el cursor de la llamada en la
línea 2 a la definición en la línea 5. Ctrl+Izquierda devolvió línea 2,
columna 8. Esta observación es distinta de la matriz automatizada.
Después Windows devolvió captura negra y GetCursorPos: acceso denegado;
el recorrido visible restante no se declara aprobado.

Comprobaciones locales actuales: formato aprobado; jc-app/source_services
14 pruebas; jc-progress/source_services 11; GUI 209 aprobadas y 15 omitidas
bajo sandbox, con dos pruebas de WinHTTP loopback bloqueadas por la red.
Las tres pruebas del módulo HTTP se repitieron fuera del sandbox y pasaron.
TUI 198 aprobadas y 14 omitidas bajo sandbox; el fixture de junction bloqueado
por permisos se repitió fuera del sandbox y pasó. Esto suma 211 casos GUI y
199 TUI distintos, sin presentar las omisiones como aprobadas.
Clippy de ambos frontends, todos los targets, pasó sin advertencias.

La suite de biblioteca de jc-app pasó otros 157 casos con 4 omisiones.
La entrega de producto se identifica como 0.1.11.

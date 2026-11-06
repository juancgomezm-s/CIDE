# Rediseño visual Git aprobado — 2026-11-06

Juan aprobó las maquetas de `prototypes/git-redesign/` y pidió llevarlas a la GUI
antes de añadir más funcionalidades. Esta entrega cambia la presentación nativa,
sin ampliar las operaciones del motor Git ni modificar la TUI.

## Composición implementada

Barra única de 42 dp, controles planos, colores del tema oscuro #2E3440,
paneles diferenciados, bordes discretos y selección azul gris. La tipografía de
interfaz sigue siendo la fuente compartida Segoe UI de CIDE; el código conserva
la fuente del editor. No se crean fuentes ni repositorios durante la pintura.

La distribución inicial asigna 30 % al área de historial y 44 % al comienzo de
la comparación. En ventanas amplias, el área izquierda contiene una franja de
ubicaciones con la rama actual y el repositorio reales. Mide 140 dp desde 1200 dp
de ventana y 185 dp desde 1400, siempre que el divisor deje al menos 340 dp al
área izquierda. No se inventa un explorador completo de referencias: remotos,
etiquetas, guardados y submódulos dibujados en la propuesta siguen fuera de esta
entrega porque el proveedor actual no ofrece sus consultas.

El historial tiene acceso directo a cambios locales, títulos y autor/identificador
en dos líneas, y una guía visual de primer padre. No se presenta como un DAG.
Los archivos muestran nombre, etiqueta de estado y directorio separados. El
detalle tiene ruta, resumen breve, fuentes en una sola fila y navegación compacta.
El diff unificado/paralelo, sus barras y los divisores conservan el único modelo
y proyector existentes. Las pestañas de área solo aparecen cuando hay paneles
ocultos. Las preferencias anteriores siguen siendo legibles sin cambiar v5.

No se agregan botones de escritura Git, red ni herramientas simuladas. Cambiar
panel, fuente o distribución sigue usando acciones reales y hits versionados.
El resumen se compone con los metadatos aceptados, sin consultas desde render.

## Corrección de entrada

La nueva barra ocupa la zona donde el editor tenía pestañas. La primera prueba
HWND detectó que sus hits ocultos interceptaban clics Git. El despacho Win32
ahora omite las pestañas del editor mientras está abierto el espacio Git; el
resto de los controles conserva sus reglas. La prueba nativa vuelve a ejecutar
clics publicados sobre distribución y vista, además de arrastres y comparaciones.

## Validación y límites

Evidencias y resultados finales en `VALIDACION.md` y
`docs/evidence/g2-git-visual-redesign-2026-11-06.json`.

La suite completa de HWND se detuvo antes de Git en Guardar todo: el archivo de
destino de prueba no se publicó por acceso denegado (error 5). El mismo escenario
falló con el binario anterior `target/g2-layout-next/release`, por lo que no se
atribuye al rediseño ni se declara aprobada esa suite. Se conservan los informes
de ambos binarios y se enriqueció el mensaje del fixture con el estado del guardado.
Se añadió una sección Git explícita a la prueba nativa, con las mismas comprobaciones
Git y controles de aislamiento; la sección completa conserva todos sus requisitos.

La captura BMP propia es salida real de GDI, distinta de una maqueta y de una
captura física. La revisión visible usa Computer Use sobre un repositorio propio,
con recuperación deshabilitada, y registra por separado lo observado. No acredita
latencia física global, IME, Narrador ni cambio de monitor. No hubo CI ni push.

## Continuación

Mostrar la interfaz real y recibir observaciones antes de ampliar G3. No iniciar
historial por archivo u otras herramientas mientras se revisa este diseño.

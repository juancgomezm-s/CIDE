# M2 — Matriz de cierre

La aceptación distingue los resultados automatizados en Windows de la
interacción en un escritorio. La TUI y la GUI comparten el documento, las
transacciones, los grupos y las tareas. La validación de OpenEdge y los
proveedores de lenguaje avanzados pertenecen a M3–M4.

| Puerta | Evidencia y estado |
|---|---|
| Regresión del workspace | 654 pruebas aprobadas en la ejecución serial final. Los tres auxiliares marcados como ignorados se invocan desde sus pruebas padre. Pasaron la creación del enlace de prueba y la integración Git que fallaron en el entorno restringido. |
| TUI en terminal privada | 23 comprobaciones entre explorador, ratón, español/Markdown y recuperación periódica. Se usa ConPTY propio y archivos sintéticos. |
| GUI en Windows | 131 comprobaciones integrales con ventana propia, entre ellas cambios de DPI simulados a 96/144/192. Dos recorridos adicionales de 16 comprobaciones cada uno cubren herramientas de código y marco. Recuperación periódica confirmada en siete comprobaciones con tres procesos aislados. |
| Interoperabilidad | Pendiente probar portapapeles entre aplicaciones, IME instalado y cambio real de monitor en un escritorio de referencia. |
| Accesibilidad | El editor nativo ya expone el grupo activo mediante UI Automation. Un cliente de Windows en otro proceso verificó lectura de rangos, foco, cursor, selección y rechazo de revisiones obsoletas. Sigue pendiente comprobarlo con un lector de pantalla. |
| Rendimiento | GUI oculta, archivo UTF-8 de 4095 bytes, Windows 10.0.26300, siete muestras tras una de calentamiento. Arranque hasta respuesta del marco p95 de 31,10 ms y máximo de 2,15 MiB privados en ese punto. Faltan la medición con ventana visible, las cargas amplias y la latencia hasta pantalla física. |
| Archivos de varios GB | La prueba aislada de lectura paginada sigue vigente. La instantánea privada y el documento editable de varios GB conservan su plan separado. |

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
4. Verificar portapapeles entre aplicaciones y restauración de un borrador
   tras reiniciar. Para automatización, mantener aislado el depósito de sesión
   y usar una memoria de portapapeles inyectada.
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
  sugerido por Windows. Algunos detalles fijos, incluida la tipografía de
  los menús emergentes nativos, quedan por revisar a escala alta.
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

La siguiente puerta es la aceptación en un escritorio con lector de
pantalla e IME instalados, portapapeles entre aplicaciones y monitores de
distinta escala. Las mediciones visibles se registrarán con ese entorno.

Con UI Automation integrado, el recorrido de arranque oculto de siete
muestras dio p95 de 36,38 ms y un máximo de 2,39 MiB privados hasta respuesta
del marco. La comprobación del marco y el dibujo fuera de pantalla pasó
16 casos. Estos resultados se registran en `work/m2-uia-startup.log` y
`work/m2-uia-shell.log`.

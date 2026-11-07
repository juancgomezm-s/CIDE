# Markdown renderizado y opción marcada — 2026-11-06

[Incremento posterior: imágenes, sangrías y enlaces](M2_MARKDOWN_MEDIA_LINKS.md).
Este documento conserva la evidencia del pintor y menú iniciales.

Ver → Vista previa de Markdown alterna entre fuente y lectura, también con
Ctrl+Alt+M. La opción muestra una marca al estar activa y solo está habilitada
cuando el perfil del documento es Markdown; un .txt que comienza con # no basta.
La GUI cierra esta vista al cambiar de documento. Esc vuelve a la fuente; F5
recaptura el contexto si la revisión quedó obsoleta. TUI conserva sus rutas
modales y añade la misma marca al menú Ver.

El proyector compartido añade metadatos de bloque y estilo a los spans existentes:
encabezados, párrafos, blancos, listas/tareas, citas, reglas, código/fences, tablas
y avisos; negrita, cursiva y tachado permanecen distinguibles al envolver líneas.
No hay otra lectura de fuente para clasificar ni un segundo documento editable.
Las cuotas existentes siguen vigentes:16KiB/línea,256KiB de lectura por preparación,
128 filas/256 columnas y navegación acotada. Los bytes inválidos producen un aviso,
sin conversión destructiva. La fuente, encoding y deshacer permanecen intactos.

GUI usa fuentes nativas proporcionadas para cuerpo, énfasis y enlaces; jerarquía
de tamaños para encabezados; código monoespaciado; fondos de código/cita, franja
de cita, reglas y celdas/bordes de tabla dibujados en píxeles. Conserva #2E3440.
La paginación cuenta las filas que caben por su altura de bloque. La rueda usa
un remanente acotado propio y no mueve el caret/viewport del editor cubierto.
Los recursos de fuente se reutilizan por DPI con propiedad RAII y el pintor
consume únicamente la presentación inmutable. No hay navegador ni dependencia nueva.

Es una vista del subconjunto Markdown existente, no un motor CommonMark completo:
HTML no se ejecuta, enlaces no se abren automáticamente e imágenes siguen como
referencias textuales. No se descargan recursos durante renderizado.

Validación:13 casos dirigidos Markdown aprobados entre jc-app,GUI y TUI, seis
nuevos. Cubren semántica/estilos, wrapping, cuotas, fences, tablas, navegación,
marca de menú, rechazo fuera de Markdown y fuente/revisión intactas. Formato y
Clippy selectivo all-targets offline aprobados. Release workspace offline aprobado; la comprobación
visual por Computer Use y su bloqueo se registran en el JSON de evidencia del bloque.
Binarios:target/markdown-render-final/release; anteriores preservados. CI dormido.


[Imagen del pintor nativo](evidence/markdown-render-native.png), obtenida con
fixture sintético en un DIB propio; no es una captura física. La inspección
corrigió medición de espacios finales entre spans y quitó el rótulo del fence de
cierre; citas/código tienen fondo elevado y la tabla dibuja su cabecera en negrita.
El esquema cubierto no deja una cabecera suelta durante esta vista.

Computer Use localizó la instancia aislada y leyó su fuente por accesibilidad,
pero la captura fue negra y activar devolvió GetCursorPos: acceso denegado
(0x80070005). Una selección nueva y el reintento dieron el mismo error. No se
inyectó entrada ni se confirma la revisión de menú/render/rueda en escritorio.
Se solicitó al usuario dejar disponible la sesión. La salida normal de esa
instancia de prueba queda pendiente; nunca reutilizar identificadores antiguos.
[Evidencia exacta](evidence/markdown-render-2026-11-06.json).

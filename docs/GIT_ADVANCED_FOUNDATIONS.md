# Git avanzado · Primer bloque del núcleo compartido

El trabajo abre la línea de Git avanzado sobre `jc-git`, compartido por los
adaptadores. En este bloque se incorporan tres contratos Rust que se pueden
validar en repositorios propios.

- **Referencias y grafo.** Consulta referencias sueltas y empaquetadas. Fija el
  inicio en HEAD o en un OID explícito y devuelve páginas de hasta 24 nodos en
  orden hijo antes de padre. El cursor retiene un grafo admitido de hasta 512
  commits. Distingue HEAD sin commit, raíz y frontera shallow. Un objeto esperado
  que falta o un grafo que supera las cuotas devuelve un error explícito.
- **Parche unificado.** Consume el modelo de comparación exacto y conserva CRLF,
  bytes heredados y la ausencia del salto final. Distingue alta, baja y cambios
  de modo. Rechaza rutas inseguras, binarios y separadores CR aislados. El máximo
  de salida es 8 MiB para fuentes de hasta 4 MiB combinadas.
- **Preparar y retirar del índice.** Un plan fija el par revisado, HEAD y el
  índice. Admite archivos enteros y selección de bloques para archivos regulares
  con el mismo modo. La confirmación usa `index.lock`, relee las identidades,
  sincroniza el índice candidato y lo publica como una unidad. El árbol de
  trabajo queda intacto. Un fallo antes de publicar puede dejar un objeto Git
  inalcanzable. El índice anterior conserva su validez.

La escritura usa repositorios ordinarios y fuentes acotadas. Los estados de
integración en curso, índices no admitidos y políticas de conversión requieren
su propio soporte antes de habilitar la acción. El adaptador debe limitar la
oferta a archivos elegibles del estado Git y proteger los buffers sin guardar.
La selección por bloques usa los rangos exactos de la comparación revisada.

Las pruebas de núcleo cubrieron cuatro casos de referencias/grafo, seis casos
de parche y tres flujos de índice. El índice se probó con un archivo nuevo, una
baja, bloques CRLF, un índice cambiado, un lock externo y cancelación antes de
publicar. El caso de índice ausente compara la misma identidad de directorio al
adquirir el lock, porque crear el lock cambia sus fechas. Las lecturas normales
conservan la comparación completa del token de ausencia.

En la GUI, abre la comparación **En disco** o **Preparados** de un archivo.
**Preparar archivo** o **Retirar archivo** solicita un plan para el archivo
entero. Revisa la comparación y elige **Confirmar índice**. Ctrl+Mayús+I activa
la acción visible. Si cambió la comparación, hay que actualizarla antes de
confirmar. El adaptador exige guardar los buffers modificados al preparar y al
confirmar. La operación en curso conserva su resultado aunque cambie el proyecto.

**Exportar parche** o Ctrl+Alt+E funciona desde las comparaciones locales e
históricas aceptadas. Elige un `.patch` nuevo fuera del repositorio. El destino
se publica sin reemplazar un archivo existente. La tarea reutiliza la misma
captura que se muestra y valida su vigencia después del diálogo. Los worktrees
con directorio Git compartido quedan fuera de esta entrega.

Se comprobaron además la confirmación de índice en el adaptador, la publicación
de parches y la preparación de un archivo desde el estado de la GUI. Las
pruebas usaron repositorios propios y comprobaron la integridad de las fuentes.
Pasaron también 25 comprobaciones con la ventana nativa oculta. La GUI se
ejecutó con PATH vacío y los hashes de los archivos y metadatos del repositorio
de prueba coincidieron antes y después. Se verificaron formato, Clippy de los
crates afectados y compilación release offline. Los binarios están en
`target/git-advanced-final/release/`. La prueba del atajo de exportación
verificó el despacho de la acción, y la prueba del adaptador verificó la
publicación. El diálogo de exportación no se abrió durante la validación.

En la TUI, abre una comparación local y pulsa I para revisar el archivo
completo. Intro confirma y Esc descarta. La prueba dirigida preparó y retiró
un archivo. También comprobó la protección de buffers modificados y los
cambios de selección antes y después de confirmar. Pasaron formato, Clippy
de la TUI y compilación release offline. Entrega en
`target/git-tui-index-next/release/`.

## Preparar o retirar un bloque

Abre **En disco** para preparar un bloque o **Preparados** para retirarlo.
En la GUI, elige un bloque con las flechas de la comparación y pulsa
**Preparar bloque** o **Retirar bloque**. **Ctrl+Alt+B** abre la revisión
del bloque visible. **Confirmar bloque** o **Ctrl+Intro** publica la
selección. En la TUI usa **←/→** para elegir, **K** para revisar e
**Intro** para publicar. **Esc** descarta la revisión en ambas interfaces.

Cada confirmación aplica un bloque de un archivo regular que existe en
ambas fuentes y conserva su modo. La preparación de archivos completos
sigue disponible para altas, bajas y otros cambios admitidos por el motor.
La revisión liga el bloque a las identidades de las fuentes, a la ruta y
al índice. La publicación aplica los rangos exactos y conserva los bytes
de los demás bloques y del árbol de trabajo.

Las pruebas dirigidas comprobaron la selección y publicación en ambas
interfaces. La prueba de TUI también descartó una revisión al cambiar de
bloque y retiró solo uno de dos bloques preparados. Se verificaron los
bytes del índice y del archivo en disco. Formato y Clippy aprobados.

## Exportar parches desde la TUI

En una comparación Git local o histórica, enfoca el diff y pulsa **X**.
Escribe una ruta absoluta a un archivo `.patch` nuevo fuera del
repositorio y pulsa **Intro**. **Esc** cancela el diálogo. Durante la
exportación, **X** solicita cancelar antes de publicar el archivo.
La tarea usa el mismo par revisado y el mismo guardado seguro que la GUI.
La exportación admite comparaciones de una sola ruta.

Quedan pendientes la gestión de referencias existentes, la restauración
de archivos, la integración de ramas y los remotos.
El [visor del grafo](G1_GRAPH_VIEW.md) recorre la ascendencia elegida y
permite crear ramas y etiquetas ligeras.
El commit local y su modificación tienen su propio recorrido de
[revisión y publicación](G4_CREATE_COMMIT.md).

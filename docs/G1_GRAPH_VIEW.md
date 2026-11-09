# Grafo de commits en GUI y TUI

Abre **Git** en la GUI o **Estado Git** en la TUI y pulsa **G**. En la GUI
también puedes elegir **Grafo**. Los carriles muestran la ascendencia de la
referencia seleccionada. **◆** marca una integración y **○** marca un corte de
ascendencia. La fila muestra el SHA abreviado, el resumen y el autor.

Pulsa **R** o elige **Referencias** en la GUI para seleccionar **HEAD**,
una rama local o remota, o una etiqueta. **Intro** abre la referencia
seleccionada y **Esc** cierra la lista. Se admiten etiquetas ligeras y
anotadas que resuelven a un commit. Si una etiqueta apunta a otro tipo
de objeto, el visor indica el fallo y conserva la página anterior.

Usa las flechas y **Intro** para abrir el commit seleccionado. En la GUI
también puedes pulsar la fila. Desde los archivos del commit, **Esc**
regresa a la misma página y selección. En la TUI, si estás en el diff,
**Esc** devuelve primero el foco a la lista de archivos. **N** muestra
la siguiente página. **F5** vuelve a consultar HEAD.

En un merge, pulsa **P** para recorrer sus padres. La GUI también ofrece
el botón **Padre 1/2 ▸**, con la posición y el total del merge abierto.
Cada elección recalcula la lista de archivos frente a ese padre. **Intro**
sobre un archivo compara exactamente esa misma base. El primer padre
es la elección inicial y se admiten hasta ocho padres.

El visor conserva hasta 512 commits de la ascendencia elegida y los
presenta en páginas de 24, con cada hijo antes de sus padres admitidos.
Los carriles continúan al pasar de página. La cabecera distingue el
límite de ascendencia, una copia shallow y las abreviaciones de texto o
carriles.

El núcleo fija la ascendencia con el identificador observado en el
selector. Si la referencia se mueve, la consulta abierta sigue el
identificador elegido. La GUI y la
TUI usan el trabajador de revisiones existente. Al abrir un commit, la
página queda disponible para volver sin otra consulta. La lectura y la
presentación del grafo se hacen fuera del editor y conservan las fuentes.

Las pruebas dirigidas comprobaron la bifurcación y unión de carriles,
el corte por cuota, la navegación a un merge en la segunda página y el
regreso a la selección. La prueba del núcleo distinguió también una
frontera shallow de un objeto faltante.

Formato, Clippy y compilación optimizada offline aprobados. La GUI pasó
26 comprobaciones con ventana oculta, incluido el pintado del grafo y
el regreso desde el detalle. La TUI optimizada pasó el recorrido del
grafo en un ConPTY privado, además de preparar un bloque y modificar el
último commit. CIDE se ejecutó con PATH vacío en ambas pruebas.
Entrega en `target/git-graph-next/release/`.

La ampliación de referencias y padres pasó pruebas del núcleo, del
controlador y de ambas interfaces. Se verificaron la apertura de una
rama que se movió después de mostrarse, una etiqueta anotada, el
rechazo de una etiqueta a un blob y el cambio de la lista y el diff
al elegir el segundo padre.

La entrega optimizada está en `target/git-refs-parents-next/release/`.
Pasaron formato, Clippy, 26 comprobaciones de regresión con la GUI
oculta y el recorrido de rama, etiqueta anotada y segundo padre en
la TUI mediante ConPTY privado. Las fuentes y los metadatos de los
repositorios de prueba conservaron sus bytes.

## Crear referencias desde un commit

En el grafo, selecciona un commit y pulsa **B** para crear una rama
local o **T** para crear una etiqueta ligera. La GUI también ofrece
los botones **Crear rama** y **Crear etiqueta**. Escribe el nombre
y pulsa **Intro** para revisar el nombre completo y el SHA de destino.
En la GUI, **Ctrl+Intro** confirma. En la TUI, usa **F2**. **Esc**
descarta el formulario.

El motor crea una referencia nueva bajo el bloqueo Git y comprueba
colisiones con referencias sueltas o empaquetadas. La creación conserva
la rama activa, HEAD, el índice, los archivos de trabajo y los reflogs.
El grafo actualiza el selector al terminar.

## Renombrar o eliminar una referencia

Abre **Referencias** con **R** y selecciona una rama local o etiqueta.
En la GUI puedes seleccionar la fila con el ratón y pulsar **Abrir**,
**Renombrar** o **Eliminar**. Ambas interfaces ofrecen **Intro** para
abrir el grafo de la referencia, **F2** para iniciar un renombrado en Windows y
**Supr** para iniciar una eliminación. **Intro** prepara la revisión
con el nombre y el objeto exactos. Confirma con **Ctrl+Intro** en la
GUI o **F2** en la TUI. **Esc** descarta la revisión.

La gestión local protege la rama activa y conserva el objeto original
al renombrar, incluso en etiquetas anotadas. Eliminar una rama requiere
demostrar que su punta está integrada en HEAD. Ramas con configuración
propia, alias simbólicos y repositorios con árboles vinculados quedan
fuera de este flujo. En caso de error tardío de E/S al renombrar pueden
quedar ambos nombres. El resultado indica que se debe actualizar el
repositorio antes de reintentar.

## Cambiar de rama local

En **Referencias**, selecciona la rama local de destino y pulsa **C** o
**Cambiar rama** en la GUI. **Intro** muestra los nombres y commits de
origen y destino y las rutas que se van a añadir, modificar o retirar.
Confirma con **Ctrl+Intro** en la GUI o **F2** en la TUI. **Esc** cierra la
revisión. CIDE vuelve a leer el grafo y el estado después del cambio.

En Windows, si los árboles coinciden, CIDE solo cambia HEAD. Si son distintos,
actualiza las rutas revisadas y sus entradas del índice. Los cambios preparados,
modificados o sin seguimiento en otras rutas se conservan. Una edición local
en una ruta afectada impide la operación. Guarda los documentos modificados
del editor antes de continuar. Si tenías abierto un archivo actualizado, vuelve
a abrirlo para ver la versión de la nueva rama.

La confirmación comprueba HEAD, ambas ramas, el índice y los archivos afectados.
Antes de la primera escritura, CIDE guarda `.git/cide-switch.pending`.
Los archivos retirados o reemplazados quedan en respaldos
`.git/cide-switch-*.bak`. CIDE publica el índice y después HEAD. Con reflog
habilitado se registra el cambio con la identidad local.

Si falla después de crear el registro, vuelve a seleccionar la rama de destino
y confirma **Reanudar cambio**. CIDE verifica las rutas ya actualizadas y
continúa con las pendientes. Si encuentra datos distintos de los revisados,
detiene la reanudación y conserva los archivos y el registro. Una ruta
reemplazada que falta y tiene respaldo requiere revisión manual para distinguir
una interrupción entre movimientos de una eliminación posterior. Si decides
volver al estado de origen en esa ruta, mueve el respaldo indicado de vuelta
a su ruta original antes de reanudar. Un bloqueo de Git que quedó tras un cierre
abrupto se informa y requiere comprobar su propietario antes de resolverlo.

El cambio de árbol admite hasta 128 rutas de archivos regulares de modo
100644 y crea las carpetas necesarias. Requiere un índice ordinario y archivos
sin conversiones. CIDE admite atributos estables que dejan los bytes intactos,
incluido `text eol=lf` en contenido LF. Los cambios de atributos, enlaces
afectados, cambios entre archivo y carpeta, alias de rutas y repositorios con
otros árboles de trabajo quedan fuera del alcance.

La suite local en Windows pasó 895 pruebas con
`python scripts/check-local.py --steps fmt test clippy build demo`.
Incluyó la vista previa y confirmación en ambas interfaces, conflictos con
ediciones locales y reanudación tras interrumpir la escritura de archivos
o del índice. Después de ampliar el soporte de atributos, pasaron siete
pruebas dirigidas y `--steps fmt clippy build demo`.

La entrega en `target/git-switch-trees-final/release/` pasó 26 comprobaciones
con una ventana oculta. En una terminal aislada, la TUI compilada cambió
entre árboles con atributos y conservó los bytes locales y su entrada
preparada. Ambos procesos se validaron con PATH vacío.

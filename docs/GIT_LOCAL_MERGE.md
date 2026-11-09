# Fusión local de ramas en CIDE

En **Git → Grafo → Referencias**, selecciona la rama que quieres incorporar a
la activa. En la GUI usa **Fusionar rama** o **M**; en la TUI usa **M**.
**Intro** abre la revisión. La revisión identifica las dos ramas y sus commits,
el resultado previsto y los archivos que cambiarían. Confirma con
**Ctrl+Intro** en la GUI o **F2** en la TUI. **Esc** cierra la revisión.

El núcleo Rust embebido calcula la ascendencia y distingue:

- **Ya incorporada:** la rama activa contiene el commit entrante. No hay
  confirmación ni escritura.
- **Avance directo:** la referencia activa se adelanta al commit entrante.
- **Ramas divergentes:** se calcula una base común y se crea un commit con dos
  padres, primero el activo y luego el entrante. En archivos regulares, el
  motor puede combinar ediciones de líneas no superpuestas. Conserva los bytes,
  incluido CRLF y la ausencia de salto al final.
- **Conflicto:** abre cada ruta con **Intro**. **Tab** recorre **Base**,
  **Actual**, **Entrante** y **Resultado**. **Ctrl+U** toma la versión
  que estás viendo como resultado. En la GUI puedes usar también los botones.

**Actual** corresponde al commit de la rama activa. **Resultado** empieza con
esa versión. Puedes editar y pegar texto,
usar **Intro** para un salto de línea y **Ctrl+T** para un tabulador.
Acepta el archivo desde **Resultado** con **Ctrl+Intro** en la GUI o **F2**
en la TUI. En la lista, la misma tecla genera la revisión final cuando todos
los archivos están resueltos. La publicación requiere confirmar desde esa
revisión. **Esc** vuelve al borrador de resoluciones desde la revisión final.

La edición admite UTF-8 de hasta 64 KiB por archivo. Para archivos mayores
o con otros bytes puedes elegir una versión completa, incluida una versión
en la que el archivo no existe.

En una colisión **Archivo/carpeta**, cada pestaña indica si la ruta contiene
un archivo, una carpeta o está ausente en ese commit. La vista de carpeta
muestra los archivos que incluye. **Ctrl+U** elige la versión completa y
**Resultado** muestra qué estructura quedará al confirmar.

Para convertir una carpeta en archivo, CIDE verifica los archivos que
retirará y guarda sus originales en `.git`. La operación se detiene si
detecta cambios locales, archivos sin seguimiento o carpetas ajenas dentro
de la ruta. Solo retira carpetas vacías. La recuperación vuelve a comprobar
los respaldos y continúa desde el último estado verificable.

En un conflicto **Renombre**, **Base**, **Actual** y **Entrante** muestran
la ruta del mismo archivo en cada versión. Con **Ctrl+U** elige la ruta
visible. **Resultado** muestra la ruta que quedará, o su ausencia si la
versión elegida eliminó el archivo. La revisión final muestra las altas y
bajas antes de escribir.

La vista previa no escribe objetos ni toca el índice. La confirmación vuelve a
comprobar las referencias, HEAD, el índice y los archivos afectados. Las
ediciones sin guardar en CIDE bloquean la operación. Una edición en disco, un
cambio preparado o un archivo sin seguimiento que coincidan con una ruta que
se debe reemplazar bloquean la escritura; el resto de los cambios locales se
conserva. En una fusión divergente, los cambios preparados ajenos quedan en
el índice y no entran en el commit de fusión.

Las resoluciones se guardan en el borrador mientras está abierto. El resultado
final se registra con su árbol exacto al confirmar. Si una publicación
interrumpida se reanuda, usa ese mismo árbol, incluidas las ediciones del
borrador.

Si falla una publicación después de crear el registro de recuperación, elige
la misma rama entrante para **Reanudar**, o usa **Abortar** desde
el grafo (**A** en GUI y TUI). Mientras el merge escribe, **Esc** pide que se
detenga en el siguiente punto seguro. Si queda pendiente, **Intro** abre la
revisión del aborto. La revisión enumera las rutas que se revertirán e indica
si ya se publicaron el índice o la rama. Confirma con **Ctrl+Intro** en la GUI
o **F2** en la TUI.

El registro y los respaldos se
guardan dentro de `.git`. El motor comprueba los identificadores, las huellas
del índice y los bytes esperados antes de retomar. Si encuentra una edición
posterior en una ruta afectada o un índice cambiado por otra operación, detiene
el aborto y conserva los cambios y respaldos. Para retirar una salida escrita
por el merge exige tanto los bytes esperados como la identidad del archivo.
Si el índice ya se publicó, recupera la copia anterior byte por byte, incluidos
los cambios preparados antes de fusionar. Si la rama avanzó, vuelve al commit
anterior tras comprobar que todavía apunta al resultado de esta fusión.

Una vez iniciado el aborto, una interrupción permite volver a revisarlo y
continuar en el sentido de la restauración. Las otras escrituras Git de CIDE
quedan bloqueadas mientras exista el registro. Si faltan pruebas de propiedad
del cambio, por ejemplo en una fusión anterior a esta función o cuando falta
un archivo y su ausencia también podría ser una eliminación local, CIDE
conserva los respaldos y se detiene. El comando opera sobre fusiones pendientes
iniciadas en CIDE, cherry-picks, reversiones, resets y rebases. El reflog de la rama y
el de HEAD registran los movimientos cuando están habilitados. El estado Git,
el grafo y el explorador se actualizan al terminar.

El flujo requiere un repositorio ordinario de Windows, una rama activa, un
índice presente y rutas admitidas por la transacción de archivos. El recorrido
de ascendencia está acotado a un máximo de 8192 commits y la operación admite
hasta 128 rutas afectadas. Varias bases de fusión, árboles sin base común,
repositorios parciales, enlaces en rutas afectadas, conversiones de texto y
conductores de merge requieren otro flujo. El conflicto de renombre requiere
una correspondencia única y contenido idéntico entre el origen y el destino.
Las copias ambiguas, los renombres con cambios de contenido y las rutas que
solo difieren por mayúsculas quedan fuera de esta resolución. El motor no
ejecuta `git.exe`, hooks ni acciones de red.

Validación local en Windows con Rust 1.88.0: `python scripts/check-local.py
--steps fmt clippy test --jobs 2` pasó con 900 pruebas y tres auxiliares
marcados como ignorados en la ejecución directa, invocados por sus pruebas
padre. La primera corrida se detuvo porque el entorno restringido no permitió
un enlace duro en una prueba preexistente; la corrida completa posterior pasó
con acceso al fixture. Se verificaron la revisión y confirmación en ambos
frontends, los conflictos, los cambios locales, la obsolescencia y la
reanudación tras publicar el índice. Las comprobaciones se ejecutaron en
segundo plano, sin escritorio, repositorios personales ni GitHub Actions.

Para generar los ejecutables usando la caché compartida:
`python scripts/build-cide-local.py --output target/git-merge-rename-next/release`.

La resolución interactiva pasó la suite local de Windows con Rust 1.88.0
mediante `python scripts/check-local.py --steps fmt clippy test --jobs 2`.
Pasaron 901 pruebas. Los tres auxiliares se ejecutaron desde sus pruebas padre.
Las pruebas cubrieron la edición y confirmación en ambos frontends, el rechazo
de cambios locales posteriores a la revisión y la recuperación del resultado
editado tras publicar el índice.

La validación de archivo/carpeta pasó 902 pruebas con el mismo comando.
Los recorridos de GUI y TUI cubrieron las dos conversiones. El núcleo
comprobó el rechazo de ediciones locales, la protección de un archivo
creado durante la publicación y la recuperación posterior.

La validación del renombre pasó 903 pruebas con el mismo comando. La GUI
conservó la ruta entrante y la TUI recuperó la ruta original. El núcleo
comprobó las tres rutas, una eliminación, el rechazo de copias ambiguas,
la protección de cambios locales y la reanudación tras una interrupción.

La validación del aborto pasó formato, Clippy y 904 pruebas locales con
`python scripts/check-local.py --steps fmt clippy test --jobs 2` en Windows.
Los tres auxiliares marcados como ignorados se ejecutaron desde sus pruebas
padre. En GUI y TUI un bloqueo de archivo interrumpió la publicación después
de escribir la primera ruta. Las pruebas abrieron la revisión y confirmaron
que el aborto retiró solo esa salida. El núcleo comprobó el aborto tras
publicar el índice y mover la rama, la reanudación de un aborto interrumpido,
las dos transiciones de archivo y carpeta, y la protección de una edición
posterior y de un archivo ajeno con contenido idéntico. Las pruebas usaron
repositorios temporales en segundo plano.

## Rebase local

En **Git → Grafo → Referencias**, selecciona otra rama local y pulsa **E**
o el botón **Rebase** de la GUI. **Intro** abre una revisión con la rama
activa, la nueva base, los commits en orden de reaplicación y los archivos
que cambiarán. Confirma con **Ctrl+Intro** en la GUI o **F2** en la TUI.

CIDE calcula toda la secuencia antes de escribir. Esta primera versión
admite hasta 32 commits lineales sobre una base común única. Conserva
autores y mensajes al generar los commits nuevos. Si un cambio ya está
aplicado y la reaplicación queda vacía, la revisión lo indica. Si hay un
conflicto, muestra el commit y las rutas afectadas. Pulsa **Esc** para volver
al grafo sin cambiar el repositorio.

Al confirmar, CIDE vuelve a verificar las dos ramas, el índice y los
archivos afectados. Bloquea las colisiones con cambios locales y conserva
las ediciones y las entradas preparadas de las rutas ajenas al rebase.
Los documentos sin guardar bloquean la operación. Si se interrumpe la
escritura, pulsa **A** en el grafo para revisar y confirmar el aborto antes
de volver a intentar el rebase. CIDE restaura la rama, el índice y los
archivos que alcanzó a cambiar. Si detecta una edición posterior, detiene
el aborto y conserva los respaldos.

En Windows pasaron formato, Clippy y 917 pruebas con
`python scripts/check-local.py --steps fmt clippy test --jobs 2` y
`RUST_TEST_THREADS=2`. Los tres auxiliares se ejecutaron desde sus
pruebas padre. La GUI verificó la vista previa y la confirmación sobre
otra rama. La TUI verificó la protección de una edición local y el aborto
tras una escritura parcial. El núcleo comprobó la reaplicación de dos
commits en orden, el conflicto sin escrituras y la restauración de la
rama, el índice y los archivos. Las pruebas usaron repositorios temporales
en segundo plano.

## Reset soft y mixed

En **Git → Grafo**, selecciona el commit de destino y pulsa **X** o el
botón **Reset**. Elige el modo con **Tab**, **↑↓** o con el ratón en la
GUI. **Intro** muestra la rama, su commit actual, el destino y las rutas
afectadas. Confirma con **Ctrl+Intro** en la GUI o **F2** en la TUI.

- **Soft** mueve la rama activa y conserva el índice byte por byte.
  La lista muestra cómo cambia la base de comparación entre commits.
- **Mixed** también ajusta el índice al árbol del destino. La lista
  muestra los cambios del índice, incluso si el destino es el commit
  actual. Si cambia el índice, CIDE conserva una copia exacta del anterior
  en `.git` y muestra su nombre al finalizar. La copia referencia objetos
  de Git y su recuperación depende de que sigan disponibles.

Ambos modos conservan los archivos de trabajo, incluidos los no rastreados,
y permiten mantener documentos sin guardar. Si la rama o el índice cambian
después de revisar, CIDE detiene la publicación. Si se interrumpe una
publicación ya confirmada, vuelve a elegir el mismo modo y destino para
reanudar o pulsa **A** para revisar el aborto. El aborto restaura el índice
y la rama si corresponde. Mixed requiere un índice existente. El historial
de la rama y de HEAD registra el movimiento si el reflog está habilitado.

En Windows pasaron formato, Clippy y 912 pruebas con
`python scripts/check-local.py --steps fmt clippy test --jobs 2`. Los tres
auxiliares se ejecutaron desde sus pruebas padre. La GUI confirmó soft con
el índice intacto. La TUI confirmó mixed con un archivo cuya versión en disco
difería de la preparada, y comprobó la copia del índice anterior. El núcleo
verificó soft sin índice, mixed sobre el commit actual y el aborto después de
publicar el índice pero antes de mover la rama. Las pruebas usaron repositorios
temporales en segundo plano.

## Reset hard

En **Git → Grafo**, selecciona el destino y pulsa **X**. Elige **hard** y
pulsa **Intro**. La revisión distingue los archivos que se reemplazarán,
crearán o retirarán del disco de los cambios que solo afectan al índice.
Confirma con **Ctrl+Intro** o **Confirmar reset hard** en la GUI, o con
**F2** en la TUI. Los documentos sin guardar bloquean la operación para
que puedas decidir qué guardar antes de confirmar.

Antes de tocar el directorio de trabajo, CIDE guarda una carpeta
`.git/cide-hard-…` y registra su ruta en el resultado. Para recuperar
contenido después de un reset completado, abre `manifest.tsv` en esa
carpeta y copia a un lugar seguro los archivos que necesites:

- Una fila **archivo** da el número de la copia en `archivos/` y la ruta
  original. Un guion en el campo de la huella anterior indica que el
  archivo se creó durante el reset y no tiene una copia anterior.
- Una fila **preparado** da el identificador de la copia en `preparados/`
  y su ruta original. Contiene los bytes preparados, aunque sean distintos
  de los que estaban en el disco. Una **eliminación preparada** queda
  registrada en el manifiesto y en el índice.
- El archivo `index` es la copia exacta del índice anterior. El manifiesto
  también indica los commits de origen y destino.

Si la publicación se interrumpe, pulsa **A** desde el grafo para revisar y
confirmar el aborto. El aborto restaura el disco, el índice y la rama según
lo que se hubiera publicado. Si detecta una edición posterior o un estado
ambiguo, conserva los archivos y el respaldo para que puedas revisarlos.
Después del aborto se conserva la carpeta de respaldo.

Este flujo admite archivos regulares con un índice existente. CIDE conserva
los archivos no rastreados y bloquea las colisiones con ellos, las
conversiones de texto no admitidas y las colisiones de archivo con carpeta.
La revisión está acotada a 128 cambios en disco, 2 MiB por archivo y un
presupuesto total de lectura y respaldo. El manifiesto se publica cuando
termina de escribirse el respaldo.

Formato y Clippy pasaron con `python scripts/check-local.py --steps fmt
clippy test --jobs 2`. La primera ejecución de pruebas se detuvo en una
prueba existente de reintentos de guardado en Windows. La repetición con
`RUST_TEST_THREADS=2` y `--steps test --jobs 2` pasó las 915 pruebas,
incluida la de guardado. Los tres auxiliares se ejecutaron desde sus
pruebas padre. La GUI verificó la confirmación y el respaldo después de
aplicar. La TUI verificó el aborto tras una eliminación seguida de un
bloqueo. El núcleo comprobó la restauración de rama, índice y disco y el
rechazo de ediciones posteriores y colisiones con archivos no rastreados.
Las pruebas usaron repositorios temporales en segundo plano.

## Cherry-pick

En **Git → Grafo**, abre la rama o etiqueta de origen desde **Referencias**.
Selecciona el commit y pulsa **P** o el botón **Cherry-pick** de la GUI.
**Intro** muestra el origen, la rama activa y las rutas que cambiarían.
Si el commit es de fusión, primero elige el padre principal con **↑↓**
o con el ratón en la GUI. Pulsa **Intro** para ver la diferencia respecto
a ese padre. **Esc** permite volver a elegir antes de publicar. Confirma
con **Ctrl+Intro** en la GUI o **F2** en la TUI.

CIDE aplica únicamente la diferencia entre el commit elegido y el padre
principal. El selector admite hasta ocho padres y muestra sus identificadores
en el orden del commit.
Para un commit raíz, usa un árbol vacío como base. El resultado tiene un solo
padre, el commit activo al revisar. Conserva el autor y el mensaje del origen
y usa la identidad local para registrar quién publica. Si el resultado no
cambia el árbol activo, no crea un commit vacío.

Si hay conflictos, usa las mismas vistas de **Base**, **Actual**, **Entrante**
y **Resultado** de la fusión. Desde la lista de conflictos, **Esc** descarta
el borrador sin escribir. Si habías elegido un padre, vuelve al selector.
Si la publicación confirmada se interrumpe después de empezar a escribir,
pulsa **A** desde el grafo para revisar y confirmar el aborto, o vuelve a
seleccionar el mismo commit para reanudar. La reanudación usa el
número y el identificador del padre que se guardaron al confirmar. La revisión
del aborto también muestra ese padre. La operación conserva los
cambios preparados y de trabajo ajenos a las rutas afectadas. Si detecta una
edición posterior sobre una ruta que debe revertir, deja el registro y los
respaldos disponibles.

La validación local de Windows pasó formato, Clippy y 906 pruebas con
`python scripts/check-local.py --steps fmt clippy test --jobs 2`. Los tres
auxiliares se ejecutaron desde sus pruebas padre. La GUI verificó el descarte
del conflicto sin escrituras y la confirmación del cherry-pick. La TUI
verificó el aborto tras una escritura parcial. El núcleo comprobó el commit
raíz, el delta seleccionado, la conservación del autor, mensaje y cambios
preparados, y la recuperación tras publicar el índice. Las pruebas se
ejecutaron en segundo plano con repositorios temporales.

La elección del padre principal pasó formato, Clippy y 907 pruebas con
el mismo comando. La GUI comprobó que el primer padre podía dar una revisión
sin cambios, que era posible volver y elegir el segundo, y que la revisión
de un conflicto conservaba esa elección. La TUI comprobó el aborto tras una
escritura parcial. El núcleo verificó que cada padre producía un delta
distinto, rechazó el cambio de padre mientras había una operación pendiente
y restauró el archivo, el índice y la rama tras interrumpir el propio aborto.

## Revertir un commit

En **Git → Grafo**, selecciona un commit y pulsa **V** o el botón **Revertir**
de la GUI. **Intro** abre la revisión de los archivos que cambiarán. Para un
commit de fusión, primero elige el padre principal. Confirma con **Ctrl+Intro**
en la GUI o **F2** en la TUI.

La operación aplica la diferencia inversa del commit respecto a su padre.
Para un commit raíz, la versión anterior es un árbol vacío. El resultado
es un commit nuevo sobre la rama activa y su mensaje identifica el original
y el padre principal, cuando corresponda. Si el árbol activo ya refleja
la reversión, CIDE muestra una revisión sin cambios.

Si hay conflictos, **Commit** muestra el contenido que se quiere revertir,
**Actual** muestra la versión de la rama activa y **Anterior** muestra la
versión del padre. Puedes elegir una versión o editar **Resultado** y volver
a revisar antes de confirmar. **Esc** descarta el borrador sin tocar el
repositorio. Si se interrumpió una publicación ya confirmada, **A** abre
la revisión del aborto. También puedes volver a ejecutar **Revertir** sobre
el mismo commit para reanudar con el padre original. CIDE conserva los
cambios locales y se detiene si detecta una edición posterior en una ruta
que tendría que revertir.

La validación en Windows pasó formato, Clippy y 910 pruebas mediante
`python scripts/check-local.py --steps fmt clippy test --jobs 2`. Los tres
auxiliares se ejecutaron desde sus pruebas padre. La GUI comprobó el descarte
de un conflicto sin escrituras y la publicación con confirmación. La TUI
restauró un archivo retirado antes de que un bloqueo interrumpiera la
segunda escritura. El núcleo verificó las reversiones de commits raíz y de
fusión, la conservación de cambios posteriores y preparados, y el rechazo
de un aborto que sobrescribiría una edición local. Las pruebas usaron
repositorios temporales en segundo plano.

# Renombrar símbolos ABL compartidos

En la GUI y la TUI, abre la carpeta del proyecto y el include guardado.
Coloca el cursor en la declaración del procedimiento, función o variable
global. Pulsa **F2**, introduce el nombre y pulsa **Intro**. La vista previa
muestra el archivo, el renglón y el reemplazo de cada aparición.

**F9** o **Ctrl+Intro** vuelve a resolver el PROPATH y compara el resultado
con la vista previa. Si cambian los archivos, las referencias o el destino,
vuelve a abrir el renombrado y revisa el alcance actualizado. Guarda primero
las ediciones ABL pendientes del proyecto para que la revisión pueda usar
las mismas fuentes que se van a modificar.

Al confirmar, todos los cambios se aplican juntos en el editor. Los archivos
afectados que estaban cerrados se abren como pestañas con cambios. **Guardar
todo** publica los archivos con el flujo de guardado habitual. **Deshacer**
o **Rehacer** desde una pestaña afectada mueve el grupo completo. Si otra
pestaña del grupo tiene una edición posterior, deshaz esa edición primero.
Mantén abiertas las pestañas afectadas para conservar el deshacer del grupo.
La recuperación de sesión guarda las ediciones y el historial agrupado en
una misma generación. El cierre normal confirma la generación final y la
recuperación periódica usa su intervalo habitual.

El recorrido usa el orden de `.cide/propath` y expande inclusiones estáticas
sin parámetros, en su propio renglón, dentro de la raíz. Conserva la ubicación
original de cada aparición aunque pase por varios includes. Compara todos
los programas consumidores para detectar si una edición compartida cambiaría
otro símbolo. Los ciclos y las resoluciones ambiguas detienen la revisión.

También sigue un handle global inicializado una sola vez con
`RUN programa.p PERSISTENT SET manejador`, resuelto por PROPATH. Incluye
`RUN rutina IN manejador`, `RUN VALUE("rutina") IN manejador` y
`DYNAMIC-FUNCTION("funcion" IN manejador, argumentos)` cuando los nombres
y argumentos son literales comprobables y la llamada pertenece al bloque
principal posterior a la inicialización. Rechaza la reasignación del handle,
su uso desconocido, las modificaciones de PROPATH en ejecución y un archivo
`.r` que oculte el fuente. El PROPATH del programa persistente y el de la
llamada deben coincidir para revisar el mismo contexto.

La revisión admite hasta 256
fuentes en 64 directorios, 256 KiB por fuente, 8 MiB acumulados, 64 reemplazos
y 16 archivos modificados. La expansión admite 16 niveles, 4096 nodos y
16 MiB acumulados. Las expresiones calculadas, macros con parámetros y
destinos que dependen del estado de ejecución necesitan ampliar el análisis.

## Validación local

En Windows con Rust 1.88.0, las pruebas dirigidas cubren la selección por
PROPATH, la vista previa y confirmación en GUI y TUI, el rechazo de un cambio
externo, y la recuperación del deshacer y rehacer de proyecto. OpenEdge 11.7
compiló dos programas consumidores después de guardar el renombrado de una
función de una cadena de includes. Se comprueba también la ejecución de
una llamada indirecta a esa función. Las comprobaciones se ejecutan en segundo
plano con fuentes sintéticas. OpenEdge 10.0B queda para la validación final.

El bloque ampliado valida 15 casos dirigidos de renombrado, incluidos dos
con OpenEdge 11.7, y seis casos de recuperación e historial. La prueba GUI
confirma la llamada mediante handle y rechaza una vista previa cuyo consumidor
cambió. La prueba TUI revierte con una sola acción los cambios del programa
y de los dos includes. El proveedor comprueba ciclos, contextos compartidos
incompatibles y llamadas a dos programas que declaran el mismo nombre.

Se ejecuta el entorno y las cachés del ejecutor local con los siguientes
argumentos de Cargo, en serie.

```console
cargo fmt --all -- --check
cargo test --locked --offline --lib -p jc-progress -p jc-gui -p jc-tui rename -- --include-ignored
cargo test --locked --offline --lib -p jc-app workbench::recovery
cargo test --locked --offline -p jc-progress --features source-services --test source_services
cargo clippy --locked --offline --all-targets -p jc-progress -p jc-gui -p jc-tui -- -D warnings
```

Los casos AVM usan `CIDE_TEST_PROGRESS_EXE` con la instalación local 11.7.
La comprobación de r-code sigue la prioridad de resolución documentada por
Progress. [Referencia de RUN](https://documentation.progress.com/output/ua/OpenEdge_latest/pdsoe/PLUGINS_ROOT/com.openedge.pdt.langref.help/rfi1424920323961.html).

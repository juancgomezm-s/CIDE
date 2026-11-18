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
en su propio renglón, dentro de la raíz. Admite argumentos nombrados y
posicionales y su reenvío explícito entre includes. Conserva la ubicación
original de cada aparición aunque pase por varios includes. Compara todos
los programas consumidores para detectar si una edición compartida cambiaría
otro símbolo. Los ciclos y las resoluciones ambiguas detienen la revisión.

Por ejemplo, un programa puede contener `{puente.i "calcular"}` seguido
de `RUN calcular.`. El puente contiene
`{rutinas.i &nombre={1}}` y el include final declara
`PROCEDURE {&nombre}:`. Al renombrar desde esa declaración, la vista previa
muestra el argumento del programa y la llamada que deben modificarse.
Las comillas dobles que delimitan el argumento permanecen en el fuente.
Las comillas simples forman parte del texto que ABL sustituye.

Cada include recibe sus propios parámetros. La revisión exige un valor
explícito para cada referencia y comprueba el efecto de volver a expandir
todo el proyecto después de los reemplazos. Si un argumento también cambia
un texto o símbolo ajeno, la operación se rechaza. Cuando la plantilla
permanece intacta y los cambios pertenecen a sus argumentos, el editor
activa el primer archivo modificado para permitir Deshacer inmediatamente.

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
16 MiB acumulados. Cada inclusión admite hasta 32 argumentos, con 4096 bytes
por valor original y expandido. Los argumentos nombrados y posicionales
deben pasarse por separado. Los nombres duplicados, argumentos ausentes,
escapes que transforman el valor, `{0}`, `{*}`, `{&*}`, nombres dinámicos
de includes y símbolos construidos con fragmentos de distintos orígenes
necesitan ampliar el análisis.

## Validación local

En Windows con Rust 1.88.0, las pruebas dirigidas cubren la selección por
PROPATH, la vista previa y confirmación en GUI y TUI, el rechazo de un cambio
externo, y la recuperación del deshacer y rehacer de proyecto. OpenEdge 11.7
compiló dos programas consumidores después de guardar el renombrado de una
función de una cadena de includes. Se comprueba también la ejecución de
una llamada indirecta a esa función. Las comprobaciones se ejecutan en segundo
plano con fuentes sintéticas. OpenEdge 10.0B queda para la validación final.

El bloque de parámetros verifica los rangos físicos, las comillas, los
límites y el reenvío. La prueba real de OpenEdge 11.7 pasa un nombre y un
tipo por posición, los reenvía por nombre y compila dos consumidores.
Después ejecuta una llamada indirecta a la función renombrada. La prueba
GUI comprueba el Deshacer inmediato cuando la plantilla está intacta.
La prueba TUI usa un valor inicial reenviado desde un parámetro posicional
en una declaración con `NO-UNDO INITIAL`. Pasaron 26 pruebas dirigidas en
Windows x64 con Rust 1.88.0, incluidas seis de recuperación e historial y
siete de servicios ABL. También pasaron formato y Clippy con avisos tratados
como errores en `jc-app`, `jc-progress`, `jc-gui` y `jc-tui`.

La compilación simultánea agotó memoria. Al compilar por separado, el
enlazador de Windows falló al generar un PDB. Las pruebas GUI y TUI usan
un solo trabajo y
`cargo rustc --lib --profile test -- -C debuginfo=0 -C link-arg=/DEBUG:NONE`
para generar sus ejecutables de prueba. Se ejecuta el mismo conjunto de
casos dirigidos mediante esos ejecutables. El argumento suprime la generación
del PDB por el enlazador. Las dependencias se reutilizan desde la caché local.

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

La interpretación de parámetros sigue las reglas de sustitución de ABL.
[Inclusiones](https://documentation.progress.com/output/ua/OpenEdge_latest/pdsoe/PLUGINS_ROOT/com.openedge.pdt.langref.help/rfi1424919802847.html)
y [referencias a argumentos](https://documentation.progress.com/output/ua/OpenEdge_latest/pdsoe/PLUGINS_ROOT/com.openedge.pdt.langref.help/rfi1424919803620.html).

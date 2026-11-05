# Estado Git accesible desde la GUI

La [integración automática posterior](M2_GIT_INTEGRADO_OMNI.md) sustituye el
selector descrito en este checkpoint. La GUI actual detecta Git y no pide buscar
el ejecutable; las instrucciones de primer uso siguientes son históricas.

Se retira únicamente la leyenda «F6 · cambiar de panel» del Explorador GUI.
La geometría y el atajo de cambio de paneles se conservan; la TUI no cambia.

## Fallos encontrados y corrección

El indicador usaba `GitStatusController::ready()` para mostrar el contador.
Ese método significa que hay una entrega esperando `poll`, no que el modelo
aceptado esté listo. Después de consumirla, el contador dejaba de aparecer.
Ahora se consulta el estado Ready y su resultado versionado, con raíz vigente.
Cerrar una vista de estado terminada conserva ese resumen; una lectura aún
en curso se puede cancelar como antes.

«Git: opcional» tampoco permitía actuar ni explicaba la falta de configuración.
El indicador muestra configurar, consultar, leyendo, limpio, número de cambios
o error. Pulsar su zona abre el flujo real de estado/configuración. Los fallos
del proveedor conservan una explicación acotada en el estado de la GUI.

La elección explícita del ejecutable se conserva ahora con la sesión GUI.
Los metadatos v3 guardan su ruta nativa; el lector admite v1 y v2 y prioriza
`--git-executable` si el usuario lo suministra. No se ejecutan binarios elegidos
por el proyecto ni se busca un Git arbitrario en PATH.

Una vez configurado, se programa consulta de estado al elegir/restaurar raíz,
guardar/Guardar como/Guardar todo y recuperar el foco de Windows. La admisión
se realiza desde la ruta de tareas, conserva una marca pendiente, el trabajador
y sus colas acotadas, y evita reiniciar una lectura activa por cada señal.
No hay proceso Git en pintura, captura documental ni vigilante permanente.

Git de estado refleja archivos en disco e índice. Los cambios sin guardar
siguen en el buffer y su indicador de pestaña; el diff de archivo activo sí
utiliza el snapshot del editor según su contrato anterior.

## Primer uso

Abrir la raíz del repositorio, pulsar «Git: configurar» y elegir el `git.exe`
instalado de confianza. En el equipo de pruebas se usó
`C:\Program Files\Git\cmd\git.exe`. La selección se recuerda con recuperación
habilitada. Pulsar el resumen vuelve a abrir/actualizar la lista de cambios.
Solo lectura; stage/commit y demás escrituras permanecen en M5.

## Evidencia local

75 pruebas GUI de biblioteca y seis de entrada aprobaron, incluidas persistencia
de Git, lectura v1/v2 y clic de configuración. Clippy de todos los destinos GUI,
formato y release offline aprobaron. El ejecutable está en
`target/git-ux-next/release/cide-gui.exe`.

El recorrido integral pasó 134 comprobaciones nativas con un repositorio propio,
incluidas entrega automática sin modal, contador después de consumir la entrega,
conservación del resumen al cerrar, índice/disco separados y diff HEAD/índice.
Fuentes y metadatos Git sintéticos quedaron intactos. Informe:
`work/native-gui-smoke/20261105T210725Z-5946738347af`.

La primera corrida restringida no pudo preparar un fixture de Guardar como.
La siguiente detectó una expectativa antigua del smoke F6 que omitía el panel
Código añadido en el bloque anterior. Se actualizó para comprobar el ciclo de
tres áreas, conservando el comportamiento de producción. No se atribuyen esos
fallos a una prueba de Git aprobada ni se borran sus informes intermedios.

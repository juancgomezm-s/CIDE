# Selección histórica al deshacer y rehacer

Bloque compartido de aplicación, posterior a `883aa19`. No añade otro motor
de edición ni duplica contenido. El núcleo de bytes sigue siendo el único
propietario del documento y su undo/redo.

## Comportamiento

Una edición lógica conserva la selección orientada de la vista iniciadora
antes y después de publicar su única transición de contenido. Al deshacer
vuelven cursor, ancla y geometría rectangular anteriores. Rehacer devuelve
los posteriores. Las otras vistas se reubican con el mapa F1; no se cambia
el grupo enfocado si el usuario deshace desde otra vista del mismo documento.

Si la vista iniciadora ya cerró, no se traslada su selección a otra identidad.
Si cambió la codificación, se conserva el mapeo y normalización vigentes sin
aplicar geometría histórica interpretada con otro codec. Los registros que
quedan fuera del límite de metadatos también usan ese comportamiento F1.

Se agrupan entrada de texto, borrados, cortar/pegar, comandos de línea,
comentarios, sangría, búsqueda/reemplazo y edición rectangular. Las plantillas
incluyen su colocación final del cursor. Propuestas para documentos inactivos
siguen sin cambiar foco ni inventar una vista iniciadora.

La revisión interactiva encontró además dos huecos de entrada. Ctrl+Inicio/Fin
y Ctrl+Mayús+Inicio/Fin ahora usan comandos/keymap compartidos, disponibles en
la paleta y solo en Editor, sin nuevas entradas de menú. Saltan directamente
a los extremos de bytes (después del BOM cuando corresponde), sin recorrer
ni decodificar el documento entero. Con Mayús conservan orientación/ancla.

También se corrige escribir tras un ancla degenerada, por ejemplo Mayús+Inicio
en una línea vacía: la inserción limpia ese ancla, de modo que los caracteres
siguientes no sustituyen accidentalmente a los recién escritos.

## Transacción y memoria

`Document::history_transitions()` expone pares de revisiones y longitudes
retenidas por préstamo, en orden de creación para undo y redo. No lee ni
copia bytes, piezas o estados de contenido. Si se elimina el predecesor de
una transición, ya no se expone como deshacible.

`SelectionHistory` guarda solo ViewId, revisiones, encoding y dos geometrías.
La reserva puede fallar antes de editar. La publicación final no asigna:
captura el resultado tras terminar el comando completo, no desde el Drop de
una proyección mutable ni desde un reemplazo interno de un rectángulo.
Los comandos anidados del mismo documento comparten ese ámbito. Su contrato
es publicar una transición core por operación lógica; no agrupa varias
mutaciones core independientes como si fueran una sola.

La capacidad es perezosa: 16 registros al necesitarla, luego se duplica hasta
1024, con cota de 256 KiB por documento. No hay reserva al abrir sin editar.
La carga de memoria se expone como `EditorDocument::selection_history_bytes`.
Es adicional a los presupuestos de historial de contenido y cachés, no una
promesa de RAM privada global. Al deshabilitar todo el historial se libera
también la tabla de selección. La poda sigue las transiciones retenidas en
una pasada ordenada; no busca todo el historial por cada registro.

## Recuperación

`CIDEAPP2` añade la tabla de metadatos, conservando `CIDEDOC1` y `CIDEPAN1`.
El lector acepta `CIDEAPP1` y deja la tabla vacía para esas sesiones antiguas.
Los lectores antiguos no interpretan el formato nuevo; usar los ejecutables
vigentes para reabrir una sesión creada por esta versión.

Las vistas restauradas reciben identidades nuevas por encima de su watermark.
El lector de paneles devuelve el mapa antiguo→nuevo; se aplica a los registros
y se valida la pertenencia de cada vista al documento. Se comprueban orden,
unicidad, revisión/transición, longitudes, flags, codec y columnas rectangulares
antes de publicar el Workbench restaurado. Las vistas cerradas se omiten al
guardar. No se aceptan referencias válidas que pertenezcan a otro documento.

Formato por registro: 144 bytes en disco; cuota de 1024 registros/documento
y límite global de metadatos de sesión de 16 MiB. La admisión del escritor y
el lector comparten los límites, incluidos los de capacidad en memoria.
La captura periódica conserva sus metadatos iniciales inmutables y el vínculo
de identidad/época del source; no cambia el worker, HEAD/BACKUP o la limpieza.

## Alcance de cierre

La evidencia local y los ejecutables finales se registran en `VALIDACION.md`.
Por instrucción de Juan, IME real queda fuera del cierre M2 solicitado; no se
instalan fuentes o idiomas ni se retira el soporte existente. Narrador sigue
aplazado. El equipo informó una pantalla física activa: no se afirma haber
trasladado CIDE entre monitores.

708 pruebas del workspace pasaron en la corrida integral final, con tres
helpers invocados por sus padres; formato, Clippy de todos los destinos y
release offline aprobaron. El modelo nuevo añadió 25 casos a las 683 pruebas
anteriores, incluidos límites de documento y campos con foco distinto.
Evidencia: `work/m2-selection-workspace-final.log` y su resumen JSON.

La medición x64 observó SelectionState de 64 bytes y SelectionRecord de
160 bytes; a 1024 registros, capacidad retenida 163 840 bytes, con crecimiento
geométrico y publicación sin asignar. `work/m2-selection-memory.log`.

139 checks nativos GUI y nueve ConPTY del binario final aprobaron. El recorrido
ConPTY encontró una omisión en su lector VT: ignoraba BS U+0008 usado por
Windows para retroceder tras borrar un carácter. Se corrigió el lector común,
con prueba pura del parser, manteniendo el oráculo estricto de EOF; no fue un
fallo del editor ni se relajó la comprobación de cursor.

GUI pasó tres reinicios normales (cuatro checks) y tres reinicios de recuperación
periódica (siete); TUI pasó otros tres periódicos (siete). Fuentes y depósitos
personales intactos/no usados. La recuperación de selección con ViewId remapeado
y deshacer/rehacer está cubierta también por el worker periódico real en Rust.

La GUI visible recorrió 360 pasos de rueda a 60 Hz solicitados sobre 4,4 MB:
mediana 3,46 ms, p95 4,17 ms y máximo 5,39 ms hasta confirmar pintura; ninguna
muestra superó 16,67 ms. No incluye fotones/compositor ni es una garantía física.
Informe `work/m2-visible-scroll-uhryviqg/report.json`.

La interacción inicial con teclado real confirmó selección Undo/Redo y cerró
normalmente. La instancia final se mostró con ShowNormal y permitió observar
el editor nativo; luego volvió el error de acceso a teclado. El último cierre
normal y la repetición final de Ctrl+Inicio/Fin de escritorio quedan pendientes,
sin alterar la fuente ni usar recuperación personal. Los detalles están en
`work/m2-selection-desktop-{initial,final}.json` y `VALIDACION.md`.

# OmniCore integrado: conversación, propuestas y edición

## Presentación y controles

El panel usa tarjetas nativas de mensajes con roles, acentos y modelo por turno,
siguiendo el presentador ConversationPresentation de OmniCore. Mantiene el tema
de CIDE y sus fuentes. El código, los encabezados, listas y estilos en línea
reutilizan el formateador Markdown compartido; no se crea otro documento ni se
cargan imágenes, páginas o destinos de la respuesta. El modelo usado queda
registrado al enviar cada turno y no cambia en mensajes anteriores al elegir otro.

Cada bloque de código cerrado de una respuesta completa ofrece **Insertar…** y
**Nuevo**. Insertar abre la comparación compartida y captura documento, revisión,
vista, cursor, selección y codificación. Aplicar vuelve a validar el destino;
un cambio local invalida la propuesta. Una aplicación es una edición ordinaria
con un solo Deshacer. Nuevo abre un documento sin guardar y no sobrescribe disco.
Los bloques incompletos y las respuestas parciales no habilitan acciones.

## Actividad técnica

En Ajustes, **Mostrar herramientas y razonamiento** está desactivado por defecto.
La preferencia se conserva en los metadatos GUI v10; v1–v9 conservan el valor falso.
El adaptador transmite registros tipados manteniendo un callback completo por
herramienta, incluso cuando su salida contiene varias líneas. La respuesta final
se separa de actividad y resúmenes. No se reconstruye ni fabrica razonamiento;
solo puede mostrarse la información compartida que entregue el runtime.

## Edición de archivos

El fallo reproducido tenía dos componentes: un turno gastó sus ocho pasos en
búsqueda de herramientas y habilidades, y el reintento mostró solamente
herramientas de lectura. El modelo tenía perfil Agente completo, pero la sesión
nueva heredaba ObserveOnly por ausencia de un valor global configurado.

La confirmación de Editar explica el alcance: crear y editar en el proyecto,
sin habilitar eliminación o ejecución general. Después de la confirmación fijada
por el controlador, el adaptador usa el mismo ITrustedUserActionClient que la UI
nativa de OmniCore: crea una conversación propia, fija ScopedCoder en esa sesión
con CAS y admite la entrada humana con modo Act y revisión de política. Ejecuta
mediante TuiTurnHost, conserva los límites del modelo y no modifica preferencias
globales. Las instrucciones del adaptador indican descubrimiento máximo ocho y
herramientas de filesystem pertinentes, sin omitir permisos ni aprobaciones.

El registro changedFiles de la misma sesión verifica el número de archivos
creados o modificados. Un turno que no escribe lo comunica. El observador de CIDE
consume los efectos confirmados: archivo nuevo como fuente; modificación con
comparación. No se sustituye un efecto de OmniCore por una diferencia Git ajena.

## Validación Windows del bloque

- 46 pruebas dirigidas OmniCore aprobadas; dos casos de entornos especiales
  omitidos. Diez pruebas de recuperación GUI aprobadas, incluida la preferencia
  nueva. Una prueba del Markdown de mensajes aprobada. Clippy app/GUI all-targets
  y compilación debug aprobados.
- Se probó Ajustes desde el escritorio: cinco proveedores y trece modelos,
  cambio gpt-6-luna → gpt-6-sol, dos respuestas reales y actualización de catálogo
  sin perder la conversación. La segunda consulta no adjuntó el archivo y
  respondió a partir del historial: contador de tipo INTEGER.
- La admisión de edición se ejercitó contra el runtime real en un workspace y
  configuración desechables, sin invocar proveedor: ScopedCoder/PatchAndCreate y
  el valor global ObserveOnly permaneció sin configurar. Esta comprobación no
  equivale a una escritura realizada por un modelo desde la GUI.
- La escritura corregida, las nuevas tarjetas, Insertar/Aplicar y el interruptor
  siguen pendientes de revisión física: Windows volvió a negar GetCursorPos y
  devolver captura negra al intentar cambiar a la GUI nueva. No se presentan
  como probados desde el escritorio ni se reemplazan por consultas ocultas.

Los registros y fixtures propios están bajo work/semantic-outline-icons. La
validación es local en Windows, sin GitHub Actions ni una ejecución nueva Linux.

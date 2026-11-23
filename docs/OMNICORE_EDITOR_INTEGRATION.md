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

La conversación nueva del runtime partía de una ruta vacía. El adaptador vincula
ahora el modelo elegido explícitamente en la confirmación de CIDE mediante
`session.model.select` y `session.model.route.respond`. Solo inicializa su sesión
nueva, sin Run activo ni selección previa; fija catálogo, revisiones, modelo,
proveedor, ruta y facturación. Mantiene la misma admisión inicial no medida del
Host (Local, IncludedQuota o CreditBalance). Una ruta medida/desconocida requiere
su autorización adicional; no se responde a interacciones arbitrarias del Run.
Sin selección explícita se pide elegir modelo antes de editar. No se accede a
métodos privados del Host ni se modificó el checkout de OmniCore.

El registro changedFiles de la misma sesión verifica el número de archivos
creados o modificados. Un turno que no escribe lo comunica. El observador de CIDE
consume los efectos confirmados: archivo nuevo como fuente; modificación con
comparación. No se sustituye un efecto de OmniCore por una diferencia Git ajena.

Si el archivo ya estaba abierto y limpio, se actualiza la sesión de archivo con
una lectura confinada de hasta 2 MiB. Conserva la pestaña, normaliza las vistas
y cambia DocumentId para invalidar tareas ligadas a los bytes anteriores. El
registro de guardado se basa en el contenido nuevo. Los buffers con cambios
locales y las vistas paginadas se conservan; se muestra un aviso y el diff sigue
siendo una comparación independiente. No se guarda ni sobrescribe desde esa vista.

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
- El 22 de noviembre se completó la prueba física con computer-use: Consulta
  devolvió una función Sumar en un bloque ABL; Insertar abrió la vista previa,
  Aplicar la incorporó al documento y un solo Deshacer lo dejó vacío. Se comprobó
  Ajustes desmarcado al iniciar, actividad visible al marcarlo y oculta al
  desmarcarlo, conservando los mensajes. Se observaron filesystem.write,
  filesystem.read y filesystem.patch reales en la actividad opcional.
- En modo Editar, gpt-6-sol creó suma-gui.p desde el composer. CIDE lo abrió
  automáticamente como código. La segunda solicitud cambió RETURN a + b. por
  RETURN (a + b) * 2. y abrió el diff rojo/verde. Esta prueba descubrió tanto
  la ruta de modelo vacía como el buffer desactualizado al cerrar el diff.
- Después de ambas correcciones, otra solicitud desde el composer cambió * 2
  por * 3. Se observó la comparación en paralelo y, al cerrarla con la ×,
  el documento mostró RETURN (a + b) * 3. La última prueba conservó los ajustes
  de apariencia elegidos por Juan. Los archivos de ejemplo son propios, bajo
  work/semantic-outline-icons/proyecto; no se editaron mediante un script para
  simular el trabajo de OmniCore.
- Tres regresiones de actualización pasaron: nueva identidad y guardado,
  cambios locales y Deshacer conservados, y rechazo por raíz/tamaño. Clippy
  app/GUI all-targets y la compilación debug pasaron después de esta corrección.
  `python scripts/check-omnicore-edit.py --runtime <runtime-local>` cerró cuatro
  casos contra el Host real sin proveedor: Local y IncludedQuota vinculados,
  MeteredCurrency y modelo inexistente rechazados antes del Run. Ninguno cambió
  la configuración global ni escribió archivos del workspace.

Los registros y fixtures propios están bajo work/semantic-outline-icons. La
validación es local en Windows, sin GitHub Actions ni una ejecución nueva Linux.

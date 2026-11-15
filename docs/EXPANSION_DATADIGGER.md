# Expansión DataDigger en CIDE

**Estado:** configuración de perfiles por proyecto en GUI y TUI. La expansión
incluye **Progress OpenEdge 10.0B y 10.2B**, **PostgreSQL** y **MySQL**, con
validación independiente por motor. El navegador de registros y los proveedores
SQL siguen pendientes.

## Experiencia en la GUI

Desde el Explorador de esquemas, la acción **Ver registros** abrirá una
pestaña de CIDE con una cuadrícula nativa, en español y con el tema activo.
La primera entrega mostrará páginas de registros, filtros por columna,
ordenación y un detalle del registro seleccionado. La cuadrícula preparará
solo las filas visibles y solicitará páginas en segundo plano.

El explorador usará el catálogo local de cada conexión. **Actualizar
esquema** minará los metadatos de la conexión elegida y publicará una
instantánea local. **Ver registros** abrirá una sesión bajo demanda con ese
mismo perfil. La conexión se liberará al cerrar la sesión o al cancelar el
trabajo que la inició.

## Configuración de conexiones

**Proyecto → Conexiones de datos** permite crear y editar perfiles en español.
El formulario muestra los campos del motor seleccionado. OpenEdge usa el perfil
Progress existente en `.cide/progress.local.ini`. Para SQL se guardan los datos
del perfil. **Probar conexión** y **Ver registros** forman parte de los siguientes
bloques de integración.

Si el proyecto tiene `.cide/progress.local.ini` y ya se guardó un perfil
OpenEdge, el selector ofrece **Actualizar esquema ABL**. En la TUI se
ejecuta con F5 sobre ese perfil. La operación reutiliza el trabajo ABL del
editor y publica el resultado en **Salida** o **Problemas**. En la TUI,
F9 guarda el formulario y Esc vuelve a la lista sin guardar el borrador.

| Motor | Configuración |
| --- | --- |
| OpenEdge | Referencia al perfil Progress existente. Reutiliza el AVM, el `.pf`, los aliases y las páginas de códigos. |
| PostgreSQL | Host, puerto, base, usuario, esquemas incluidos y configuración TLS. |
| MySQL | Host, puerto, base, usuario y configuración TLS. |

Los perfiles se guardan en `.cide/conexiones.local.json`. El archivo usa el
escritor atómico del editor y conserva el contenido si cambia en disco mientras
el formulario está abierto. El campo de credencial admite una referencia
opcional del tipo `cide/datadigger/inventario`. La consulta del almacén seguro
y el ingreso de contraseñas quedan para el bloque de conectividad. Las cuentas
SQL deberán tener permisos de lectura sobre los catálogos y datos autorizados.

```json
{
  "version": 1,
  "conexiones": [
    {
      "motor": "postgresql",
      "campos": {
        "nombre": "Inventario",
        "servidor": "db-ejemplo.invalid",
        "puerto": "5432",
        "base": "inventario",
        "usuario": "lectura_cide",
        "esquemas": "public",
        "tls": "verificar-servidor",
        "credencial": "cide/datadigger/inventario"
      }
    }
  ]
}
```

### Validación local de la configuración

En Windows con Rust 1.88 pasaron las pruebas dirigidas del almacenamiento, el
formulario nativo con ventana de prueba y el recorrido de teclado en la TUI.
Cubren alta, edición, reapertura, cancelación, validación de puertos y rechazo
de guardados sobre cambios externos. También se verificó la lectura del esquema
de una copia de Sports2000 con OpenEdge 11.7 y `-RO` desde ambas interfaces. El
catálogo conserva el índice interno `default` de `InventoryTrans` sin presentar
su componente anónimo como una columna de usuario. Los archivos de la base
conservaron sus huellas SHA-256.

## Minería de esquema SQL

Los proveedores SQL consultarán los metadatos visibles para la cuenta
configurada. El catálogo incluirá esquemas, tablas, vistas, columnas,
tipos, nulabilidad, claves primarias y foráneas e índices. El proveedor
conservará los nombres y tipos originales junto al modelo común. La
instantánea se identificará por perfil y se sustituirá al completar una
lectura válida. Una actualización fallida conservará el último catálogo
válido e indicará su antigüedad. La minería de esquema lee metadatos y
la cuadrícula solicita filas por página al explorar una tabla o vista.
En la prueba PostgreSQL 17.11, `information_schema.table_constraints`
omitió claves con un rol de solo lectura. La consulta a `pg_catalog`
recuperó la clave primaria y la foránea con esa misma cuenta.

## Qué aprovechar del DataDigger personalizado

- `DataDiggerLib.p`, `getFields` y `getPrimaryFields`: metadatos, columnas
  del índice primario y tratamiento de `EXTENT`. Reusar el catálogo de CIDE
  para identificar tablas, campos e índices. Representar cada elemento de un
  `EXTENT` con su índice y permitir verlo como `Campo[1]`, `Campo[2]`, etc.
- `wDataDigger.w`, `reopenDataBrowse`: separar la secuencia de búfer y
  consulta dinámicos del bloque que crea el widget `BROWSE`. Leer las celdas
  con `BUFFER-VALUE` y mantener el valor tipado separado de su formato.
- `getFilterQuery`: preservar la experiencia de filtros por columna. CIDE
  enviará operador, campo y valor tipado. El auxiliar verificará el esquema
  y construirá la consulta. Ante un filtro inválido, mostrará el error y
  conservará la última página válida.
- `DataDigger.i`: usar sus adaptaciones históricas como pistas de prueba.
  Verificar cada tipo y función que emplee el auxiliar en las dos AVM objetivo.
  Revisar licencias y avisos antes de incorporar código de terceros.

## Contrato de consulta

Rust manejará el ciclo de vida del auxiliar ABL, la presentación y un
contrato común de columnas, filtros, orden y páginas. El auxiliar resolverá
la base y la tabla a partir de metadatos validados. La primera entrega
admitirá consultas sobre una sola tabla, con operadores permitidos por tipo.
Los identificadores vendrán del catálogo validado y los valores se
codificarán como literales ABL según su tipo. La entrada será estructurada y
el auxiliar generará exclusivamente consultas `NO-LOCK`. La sesión usará
credenciales con permisos de lectura donde el entorno lo permita.

La salida distinguirá valor desconocido ABL (`?`), cadena vacía, tipo y
codificación. Cada respuesta tendrá cuotas de filas, columnas y bytes, y
un identificador que permita descartar resultados de filtros anteriores.
La primera página se entregará sin esperar el conteo total. La paginación
usará orden estable cuando haya un índice único compatible. Si falta un
orden adecuado, se limitará la navegación a un cursor secuencial acotado.
Las operaciones de edición, borrado e importación son una expansión aparte.

## Bloques y criterios de aceptación

1. **Contrato y auxiliar de lectura.** Probar con datos sintéticos el
   transporte, los filtros, los errores y los límites. Compilar y ejecutar
   el auxiliar por separado en 10.0B y 10.2B, con sus perfiles y páginas de
   códigos reales. Registrar versión y resultados por ambiente.
2. **Cuadrícula nativa.** Integrar la pestaña con el catálogo de esquema.
   Validar carga por páginas, filtro inválido, cancelación, pérdida de
   conexión y descarte de resultados obsoletos. Verificar que la cuadrícula
   siga respondiendo durante una consulta lenta y que la base de prueba
   conserve sus registros.
3. **Compatibilidad y cierre.** Verificar en ambas AVM campos escalares y
   `EXTENT`, valores `?`, texto con acentos, fechas, decimales, ordenación
   con claves repetidas y tablas sin índice único adecuado. Cubrir intentos
   de introducir operadores o campos no admitidos. Validar la liberación
   de consultas, búferes y conexiones al terminar o cancelar.

4. **Proveedores PostgreSQL y MySQL.** Validar conexión, configuración
   por motor, permisos, TLS, minería y refresco de metadatos con bases de
   prueba. Reusar la cuadrícula y probar consultas parametrizadas,
   distinción de `NULL` y cadena vacía, paginación estable, cancelación y
   preservación de los datos. Los nombres de objetos se resolverán desde el
   catálogo y se citarán con las reglas del motor.

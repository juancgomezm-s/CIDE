# Expansión DataDigger en CIDE

**Estado:** perfiles, cuadrícula OpenEdge con edición y respaldos, y consulta
de PostgreSQL y MySQL por ODBC en GUI y TUI de Windows. La validación en
**OpenEdge 10.0B y 10.2B** se ejecuta por separado de las pruebas de 11.7.

## Experiencia en la GUI

Después de **Actualizar esquema ABL**, la acción **Ver → Ver registros ABL**
abre un selector de tabla. También se puede abrir desde **Conexiones de datos**
con el perfil OpenEdge. La GUI usa un diálogo con una cuadrícula nativa. La
TUI muestra la cuadrícula y el detalle de celda con el teclado. Ambas consultan
páginas en segundo plano y permiten filtrar y recorrer el índice en ambos sentidos.

La consulta SQL abre una sesión bajo demanda y carga el catálogo de tablas
y vistas. Los metadatos de cada objeto se consultan al abrir **Esquema**.
**Actualizar catálogo** vuelve a leer las relaciones de la sesión. Si falla,
el visor conserva los resultados aceptados. Al cerrar el visor, el trabajador
libera la conexión cuando termina la llamada que esté en curso.

## Configuración de conexiones

**Proyecto → Conexiones de datos** permite crear y editar perfiles en español.
El formulario muestra los campos del motor seleccionado. OpenEdge usa el perfil
Progress existente en `.cide/progress.local.ini`. Para abrir una conexión SQL
guardada, elige **Consultar** en la GUI o pulsa F6 sobre el perfil en la TUI.
El visor muestra el catálogo cuando la conexión responde y el diagnóstico si
falla.

Si el proyecto tiene `.cide/progress.local.ini` y ya se guardó un perfil
OpenEdge, el selector ofrece **Actualizar esquema ABL**. En la TUI se
ejecuta con F5 sobre ese perfil. La operación reutiliza el trabajo ABL del
editor y publica el resultado en **Salida** o **Problemas**. En la TUI,
F9 guarda el formulario y Esc vuelve a la lista sin guardar el borrador.

### Lectura de registros OpenEdge

Para esta primera cuadrícula, usa una copia cerrada de la base y un `.pf`
dentro del proyecto, con una conexión local por línea. Por ejemplo:

```text
-db "C:/bases/demo/sports2000" -ld sports2000 -1 -RO
```

El lector inicia una sesión independiente para la base seleccionada. La AVM
confirma que la conexión es de solo lectura y el auxiliar desactiva los
disparadores FIND antes de abrir una consulta `NO-LOCK`. El orden usa el
índice primario único observado y revalidado en la base. Cada página admite
20 filas y hasta 12 columnas escalares. La primera versión admite un filtro
exacto de texto o entero y distingue `?` de una cadena vacía. Los filtros de
texto siguen las reglas de comparación de la AVM.

La prueba del lector con Sports2000 en OpenEdge 11.7 verificó dos páginas
de `Customer`, orden inverso, filtros de texto y entero y los bytes de la
base después de consultar. Las pruebas de GUI con ventana oculta y las de
la TUI verificaron la presentación de páginas y su interacción por separado.

Con un esquema cargado, en la TUI se puede usar F6 sobre el perfil OpenEdge.
Dentro de la cuadrícula, **RePág** y **AvPág** cambian de página, **F** aplica
un filtro, **X** lo quita, **O** invierte el orden e **Intro** abre el detalle.

| Motor | Configuración |
| --- | --- |
| OpenEdge | Referencia al perfil Progress existente. Reutiliza el AVM, el `.pf`, los aliases y las páginas de códigos. |
| PostgreSQL | Host, puerto, base, usuario, esquemas incluidos y configuración TLS. |
| MySQL | Host, puerto, base, usuario y configuración TLS. |

Los perfiles se guardan en `.cide/conexiones.local.json`. El archivo usa el
escritor atómico del editor y conserva el contenido si cambia en disco mientras
el formulario está abierto. El campo de credencial admite una referencia
opcional del tipo `cide/datadigger/inventario`. CIDE lee la contraseña de una
credencial genérica con ese nombre en el **Administrador de credenciales de
Windows**. El usuario de conexión se toma del perfil de CIDE. Configura una
cuenta con permiso de lectura sobre los catálogos y datos autorizados. CIDE
usa como opción inicial `verificar-servidor` y la confianza configurada para
el controlador.

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

Los proveedores SQL consultan los metadatos visibles para la cuenta
configurada, incluidas columnas, tipos, nulabilidad, claves primarias y
foráneas e índices. La cuadrícula admite páginas de hasta 20 filas y filtros
exactos parametrizados de texto y entero. Para avanzar páginas o invertir el
orden se requiere una clave primaria que garantice un orden estable. Las
vistas y tablas sin clave admiten una primera página. `NULL` y texto vacío se
muestran por separado.

Windows necesita un controlador Unicode de **psqlODBC** para PostgreSQL o
**MySQL Connector/ODBC** 8 o posterior para MySQL. La aplicación detecta los
controladores registrados. En este equipo se verificó el diagnóstico de
controlador ausente y el visor por separado de la conexión real.

PostgreSQL 17.11 pasó 18 comprobaciones con psqlODBC Unicode en Debian bajo
WSL. Incluyeron TLS con verificación de identidad, consultas del proveedor,
claves, filtros parametrizados, paginación, solo lectura y datos intactos.
MySQL 8.0.45 pasó 15 comprobaciones del motor en una instancia aislada
en Windows. La conexión completa desde CIDE con ambos controladores ODBC
registrados en Windows queda pendiente.

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
## Edición con respaldo y restauración

Habilita una copia cerrada de la base dentro del proyecto con
`.cide/edicion-registros.local.ini`. La base lógica y el archivo deben
coincidir con la conexión del `.pf` local mostrado arriba.

```ini
base=sports2000
archivo=sports2000.db
```

En **Ver registros ABL**, selecciona un registro. La GUI ofrece **Editar**,
**Historial** y **Fila…**. En la TUI, **E** edita la celda seleccionada y
**R** abre el historial por registro. La vista previa muestra el valor actual y
el propuesto. Confirma en el diálogo o con **F9** en la TUI. La cuadrícula
vuelve a consultar la página tras el cambio confirmado.

Rust guarda y relee el respaldo antes de iniciar la AVM de escritura.
OpenEdge localiza la fila con bloqueo exclusivo, compara la identidad y
la imagen binaria completa y aplica el cambio en una transacción. Una
restauración verifica también el recibo del último cambio confirmado y
crea su propio respaldo. El almacén está en `.cide/respaldos-registros/`.
Una operación de edición con respaldo publicado y resultado incierto aparece
como pendiente. El historial compara la fila actual con la imagen del respaldo
cuando puede leerla y muestra si coincide, difiere o está ausente. La
comparación conserva el estado pendiente. La restauración de una edición
histórica requiere seleccionar su recibo. CIDE lo revalida al confirmar y
compara el registro completo con la imagen posterior a esa edición. Si difieren, muestra el valor actual y el
esperado, y conserva el registro. [Método ABL para capturar la imagen del registro](https://docs.progress.com/bundle/openedge-abl-reference-128/page/RAW-TRANSFER-method.html).

Este bloque admite cambios de texto y entero en filas existentes con una
clave primaria entera de una columna y con imagen binaria completa. La
escritura se ejecuta sobre copias locales autorizadas y desactiva los
disparadores de búsqueda y carga. La edición con efectos de disparadores
sigue fuera del alcance verificado.

**Fila…** ofrece alta, borrado y restauración de alta o borrado por clave.
En la TUI se usan **N**, **D** y **U**, respectivamente. El alta permite
introducir valores iniciales de texto y entero. Los campos omitidos usan los
valores iniciales de OpenEdge. El respaldo del alta registra la ausencia
anterior de la clave. El borrado conserva la imagen binaria completa. Al
revertir un alta, CIDE exige que la fila coincida con la creada. Al recuperar
un borrado, rechaza la operación si la clave volvió a ocuparse y verifica
que la imagen binaria recuperada sea igual al respaldo. La restauración
estructural usa la última operación de fila confirmada para la clave.

Validado localmente con copias separadas de Sports2000 en OpenEdge 11.7. La
GUI con ventanas propias ocultas y la TUI comprobaron la cancelación, la
edición confirmada y la restauración tras volver a consultar. El núcleo
comprobó que un fallo de respaldo bloquea la escritura, que un cambio externo
en otra columna invalida la vista previa de edición o restauración y que
un respaldo alterado se rechaza al compararlo con su recibo.
Las pruebas de fila también comprobaron el alta de una clave nueva, la
reversión del alta, el borrado y la recuperación exacta. Una clave ocupada
entre la revisión y la confirmación mantuvo el registro que ya existía.
Al final del bloque se comprobó la disponibilidad de las AVM antiguas. Los
ejecutables de OpenEdge encontrados en este equipo corresponden a 11.7.
Las pruebas de ejecución con 10.0B y 10.2B quedan pendientes.

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

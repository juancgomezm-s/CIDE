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

**Proyecto → Conexiones de datos** abre los asistentes en español. Para
**OpenEdge**, indica el AVM y las páginas de códigos. Selecciona un `.pf` de
confianza o elige el archivo `.db` de una copia cerrada para que CIDE genere
los parámetros de solo lectura. Guardar crea o actualiza el INI del proyecto.

Para **PostgreSQL** y **MySQL**, CIDE propone los puertos 5432 y 3306,
respectivamente, y verifica la identidad del servidor TLS por defecto.
La GUI presenta las opciones TLS antes de los campos de servidor. En la TUI,
F4 cambia el modo cuando el campo TLS tiene el foco. Si la conexión usa
contraseña, agrega una credencial genérica en el Administrador de credenciales
de Windows con un nombre como `cide/datadigger/inventario` y usa ese nombre
en **Referencia de credencial**. El perfil guarda esa referencia. Para abrir
una conexión guardada, elige **Consultar** en la GUI o F6 sobre el perfil
en la TUI. El visor muestra el catálogo o el diagnóstico de conexión.

Antes de guardar, usa **Probar conexión** en la GUI o **F8** en la TUI.
La prueba usa los valores que están en el formulario. OpenEdge indica la
versión del AVM y las bases conectadas. Para una copia local, usa un `.pf`
temporal de solo lectura. PostgreSQL y MySQL usan el mismo controlador y
los mismos ajustes que el visor, comprueban `SELECT 1` y cierran la sesión.
Guardar sigue siendo una acción separada.

En Windows x64 pasó la prueba previa al guardado con OpenEdge 11.7 y
una copia de Sports2000. Un alias inexistente presentó el diagnóstico en
español y el proyecto siguió sin perfil hasta pulsar Guardar. PostgreSQL
17.11 con psqlODBC Unicode 17.00.0004 y MySQL 8.0.45 con Connector/ODBC 9.6
respondieron en bases desechables mediante `127.0.0.1`. Las dos rechazaron
una base inexistente. Esas conexiones locales se probaron con TLS deshabilitado.

Si el proyecto tiene `.cide/progress.local.ini`, el selector ofrece
**Actualizar esquema ABL**. En la TUI se
ejecuta con F5 sobre ese perfil. La operación reutiliza el trabajo ABL del
editor y publica el resultado en **Salida** o **Problemas**. En la TUI,
F9 guarda el formulario y Esc vuelve a la lista sin guardar el borrador.

### Lectura de registros OpenEdge

Para consultar una copia cerrada, usa un `.pf` dentro del proyecto, con
una conexión local por línea. Por ejemplo:

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
| OpenEdge | AVM, páginas de códigos, versión y aliases. Admite un `.pf` existente o genera uno para una copia local. |
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
controladores registrados. Las pruebas de GUI y TUI verifican sus visores y
el diagnóstico de controlador ausente.

PostgreSQL 17.11 pasó 18 comprobaciones con psqlODBC Unicode en Debian bajo
WSL. Incluyeron TLS con verificación de identidad, consultas del proveedor,
claves, filtros parametrizados, paginación, solo lectura y datos intactos.
MySQL 8.0.45 pasó 15 comprobaciones del motor en una instancia aislada
en Windows. También pasó la conexión real cifrada desde el trabajador Rust
de CIDE en Windows x64 con psqlODBC Unicode 17.00.0004 y Connector/ODBC
9.6.0, usando un registro privado del proceso de prueba. Ambas sesiones
consultaron catálogo y columnas, filtraron un valor con acentos y
distinguieron `NULL` de una cadena vacía en dos páginas de registros.

### Completar SQL con OmniCore

En la GUI, usa **Completar bloque con OmniCore** desde un archivo `.sql`. Coloca el
cursor en una sentencia delimitada por punto y coma o dentro de una rama
`CASE`. Con un único perfil SQL guardado para el proyecto, CIDE consulta por
ODBC los nombres y tipos de columnas de las tablas visibles que reconoce en
la sentencia. El contexto del modelo usa esos metadatos y el fragmento activo.

Revisa la inserción en la comparación antes de aplicarla. **Ctrl+Z** revierte
el cambio en un solo paso. La propuesta modifica el documento y su ejecución
queda a cargo del usuario.

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
edición directa se ejecuta sobre copias locales autorizadas y desactiva los
disparadores de búsqueda y carga.

**Fila…** ofrece alta, borrado y restauración de alta o borrado por clave.
En la TUI se usan **N**, **D** y **U**, respectivamente. El alta permite
introducir valores iniciales de texto y entero. Los campos omitidos usan los
valores iniciales de OpenEdge. El respaldo del alta registra la ausencia
anterior de la clave. El borrado conserva la imagen binaria completa. Al
revertir un alta, CIDE exige que la fila coincida con la creada. Al recuperar
un borrado, rechaza la operación si la clave volvió a ocuparse y verifica
que la imagen binaria recuperada sea igual al respaldo. La restauración
estructural usa la última operación de fila confirmada para la clave.

### Edición directa en una base activa local

En OpenEdge 11.7, CIDE admite un servidor multiusuario local en memoria
compartida. En el `.pf`, declara la base como activa:

```text
-db "C:/bases/demo/sports2000.db" -ld sports2000
```

Para habilitar la escritura, añade `modo=activa` después de `archivo=`
en `.cide/edicion-registros.local.ini`. Con `base=` y `archivo=`,
el perfil debe coincidir con la base del `.pf`. Refresca Esquema ABL y
abre Ver registros ABL. La vista previa distingue la base activa de la
copia cerrada.

CIDE obtiene la imagen para la revisión con bloqueo compartido sin espera.
Tras confirmar, verifica el respaldo en disco, adquiere el bloqueo
exclusivo sin espera y compara la imagen completa y el identificador de la
fila. Para un alta verifica la ausencia de la clave y deja que OpenEdge
impida duplicados. Un bloqueo o una imagen distinta cancela la operación.
Para restaurar, se aplica la misma comprobación con el recibo seleccionado.

Con el perfil básico, el auxiliar comprueba los disparadores del esquema
antes de escribir dentro de la transacción. Para ejecutar los disparadores
del proyecto en la base activa, configura el PROPATH descrito abajo. [Modos de conexión ABL](https://docs.progress.com/bundle/openedge-abl-manage-applications/page/Connection-modes.html).

La prueba con Sports2000 11.7 mantuvo un bloqueo desde una segunda sesión
durante la confirmación. CIDE rechazó el cambio mientras la otra sesión
estaba todavía conectada. También rechazó la edición cuando otra sesión
cambió una columna distinta de la seleccionada, y el borrado cuando
detectó el disparador DELETE de Customer.
Otra copia de Sports2000, preparada con Customer sin disparadores, pasó
edición, altas, borrados y restauración en la GUI con ventanas ocultas y
en la TUI. La prueba de lectura de la copia cerrada con `-1 -RO` también
pasó y comparó los archivos de la base byte por byte.

### Disparadores y registro JSON por fila

Para editar en línea con los disparadores del proyecto, conserva
`modo=activa` en el perfil de edición y crea
`.cide/disparadores-registros.local.ini` con el directorio de código ABL
del proyecto:

```ini
propath=disparadores
```

Cada operación confirmada en DataDigger guarda un JSON en
`.cide/respaldos-registros/<base>/<tabla>/<clave>/`. Contiene la base, tabla,
clave, columna, operación, codificación y estado anterior de esa fila.
El estado anterior incluye la imagen completa en hexadecimal y el valor
visible de la columna. Para un alta, el estado anterior es `null`.
CIDE vuelve a leer el JSON antes de iniciar la escritura y genera un
recibo separado cuando OpenEdge confirma la transacción. Si falta el
recibo, la edición figura como pendiente y no se restaura automáticamente.

Abre **Historial** en la GUI o pulsa **R** en la TUI cuando necesites revisar
una edición de CIDE. Las altas y borrados se restauran por clave desde
**Fila…** o **U**. La restauración comprueba el recibo y la imagen actual
y devuelve únicamente la fila respaldada a su estado anterior. Si cambió
desde el recibo, CIDE señala el conflicto. El historial registra las
operaciones que se solicitan en CIDE. Las escrituras de otras sesiones
quedan fuera del historial, al igual que los efectos en otras filas
provocados por los disparadores. En el modo configurado, las
restauraciones respetan los disparadores de OpenEdge y exigen que la
imagen final de la fila sea igual a la respaldada. Se conservan los
lectores de respaldos de fila del formato anterior.

La prueba con Sports2000 11.7 modificó Customer en una base activa. Su
disparador actualizó otro campo de Customer y también Item. Restaurar el
respaldo JSON recuperó la fila completa de Customer y conservó el cambio
de Item. La prueba verificó que no se generara una copia de la base.

Las pruebas de ejecución con OpenEdge 10.2B y 10.0B quedan pendientes.

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

# Puente Progress

El puente evita implementar un compilador ABL propio. CIDE prepara una solicitud y una sesión de Progress ejecuta un helper ABL.

## Configuración del proyecto

Abre la carpeta del proyecto y crea `.cide/progress.local.ini` en UTF-8.
El perfil es local e ignorado por Git. Usa el cliente por lotes de tu
instalación y las páginas de códigos del proyecto. Por ejemplo:

```ini
executable=C:/Progressx86/OpenEdge/bin/_progres.exe
cpinternal=iso8859-1
cpstream=ibm850
# version=10.0B
# pf=config/desarrollo.local.pf
# ini=config/progress.local.ini
# aliases=negocio:devlogica,archivo:vacia
```

Las rutas de `executable`, `pf` e `ini` pueden ser absolutas o relativas a
la raíz. `pf` e `ini` son opcionales y se pasan explícitamente al AVM.
Configura aquí solo ejecutables y parámetros de un entorno de desarrollo de
confianza. La sesión por lotes puede acceder a los recursos que declaren el
programa y su `.pf`. Las páginas de códigos verificadas en este bloque son
ISO-8859-1 para diagnósticos y IBM850 para salida.

`version` es opcional. Fija el valor exacto de `PROVERSION` que esperas del
ejecutable, por ejemplo `10.0B` o `11.7`. **Salida** informa la versión que
ejecutó el auxiliar. Si difiere de la configurada, el puente registra el error
en **Problemas** antes de compilar, ejecutar un programa o leer el catálogo.
Las conexiones que declare el `.pf` forman parte del arranque del AVM.

Para usar una base de desarrollo, declara sus conexiones en el `.pf`.
Usa `-ld` para fijar el nombre lógico y `-RO` para abrir una base local
en modo de solo lectura. Por ejemplo, para dos copias locales:

```text
-db "C:/ruta/desarrollo" -ld devlogica -1 -RO
-db "C:/ruta/vacia" -ld vacia -1 -RO
```

Con `aliases=negocio:devlogica,archivo:vacia`, la AVM crea esos aliases
antes de comprobar, compilar, ejecutar o actualizar el esquema. Cada
destino debe ser un nombre lógico conectado. Se admiten hasta 32 aliases
y `DICTDB` se reserva para recorrer el diccionario.

`.cide/propath` es el mismo archivo que usa la navegación estática de CIDE.
Admite hasta 32 directorios relativos a la raíz, uno por línea, con `#`
para comentarios. Se respeta el orden y se admiten espacios. Usa `.` para
incluir la raíz explícitamente. Si falta el archivo, se busca primero junto al
fuente y después en la raíz. Las rutas del protocolo `JCIDE/1` deben poder
representarse en ASCII imprimible. Los archivos fuente se leen con sus bytes
originales y las inclusiones se toman de disco.

## Acciones en GUI y TUI

Guarda el archivo y elige el comando desde **Herramientas**.

- **ABL: comprobar sintaxis** (F7) compila en un directorio temporal y elimina los artefactos al terminar.
- **ABL: compilar** (Ctrl+F7) conserva el código `.r` en un directorio temporal único y muestra su ruta en **Salida**. Admite fuentes `.p`, `.w` y `.cls`.
- **ABL: ejecutar** (F5) compila y ejecuta el código `.r` recién generado para un procedimiento `.p` por lotes. El procedimiento debe retornar al puente. La salida estándar y los errores del proceso aparecen en **Salida**.
- **Actualizar esquema de base de datos** (Mayús+F7) consulta las bases del perfil y muestra sus nombres lógicos, aliases, tablas, campos e índices en **Salida**. Basta con abrir la carpeta del proyecto.
- **Cancelar tarea ABL** solicita terminar el proceso AVM propio. Las tareas vencen a los 30 segundos. Cambiar la revisión invalida un trabajo de fuente y cambiar de proyecto invalida la lectura de esquema. Una cancelación no revierte efectos que el procedimiento ya haya producido.

Los errores y advertencias del compilador aparecen en **Problemas**. Cuando
el AVM identifica una línea del documento compilado, la fila permite ir al
inicio de esa línea y muestra la columna informada. Los diagnósticos de
inclusiones u otras fuentes se muestran con la ubicación que entrega el AVM.
Solo se inicia una tarea ABL a la vez. Los resultados y la captura de texto
están acotados. El proceso se ejecuta con la raíz del proyecto como carpeta
de trabajo.

## Protocolo

Las solicitudes y respuestas usan ASCII. Los campos de texto de la
respuesta viajan en hexadecimal, con su página de códigos de origen.
`JCIDE/1` sigue disponible y `JCIDE/2` añade una línea de aliases a la
solicitud. Las consultas de esquema con `JCIDE/2` incluyen el registro
`DATABASE`, que identifica también bases sin tablas de usuario. Las respuestas
de `JCIDE/1` mantienen los registros de la versión anterior.

GUI y TUI envían `JCIDE/3`, con la versión esperada o una línea vacía después
de los aliases. Su cabecera `JCIDE|2` incluye la versión efectiva en hexadecimal.
Las solicitudes `JCIDE/1` y `JCIDE/2` siguen devolviendo `JCIDE|1`.
Los otros registros son `DIAG`, `TABLE`, `FIELD`, `INDEX`, `INDEX_FIELD`
y el terminador `END|OK` o `END|ERROR`.

El parser Rust impone límites de tamaño y rechaza respuestas incompletas o contradictorias.

## Esquema

El helper consulta `_File`, `_Field`, `_Index` y `_Index-Field` con
`NO-LOCK` mediante `DICTDB`. Solo se acepta un catálogo completo para la
carpeta activa. Una falla de conexión, alias, cuota o protocolo se muestra
en **Problemas** y conserva el catálogo anterior. **Ver → Ver esquema ABL**
vuelve a mostrarlo sin ejecutar el AVM. El catálogo vive en memoria por
proyecto, admite hasta 1 MiB y 8192 registros, y el panel muestra una vista
de hasta 256 entradas junto al conteo total. Cambiar de proyecto cancela la
lectura y libera el catálogo de la carpeta anterior.

### Autocompletado de esquema en GUI y TUI

Actualiza el esquema con **Mayús+F7**. En un archivo ABL de esa carpeta,
usa **Editar → Completar desde esquema ABL** o **Ctrl+Alt+Espacio**.
Selecciona una sugerencia con las flechas y aplícala con **Intro**.
**Ctrl+Z** revierte la inserción en un paso.

- **Tablas** en frases como `FOR EACH negocio.cust`, `FIND FIRST cust` y
  `CREATE cust`. Si el nombre aparece en varias bases, se muestra e inserta
  calificado.
- **Campos** en expresiones como `negocio.customer.na` o justo después de
  `WHERE` cuando la tabla se puede identificar.
- **Índices** después de `USE-INDEX`, usando la tabla de la frase activa.

Se admiten nombres lógicos y aliases del perfil. Se presentan hasta 32
sugerencias. La consulta corre en segundo plano
y usa hasta 2 MiB de texto anterior al cursor. Cambiar el texto, el cursor,
la vista, el proyecto o el catálogo invalida las sugerencias.

- **Buffers estáticos** de tablas de base de datos y tablas temporales.
  Por ejemplo, `bCliente.na` usa los campos de la tabla que respalda a
  `bCliente`.
- **Tablas temporales** con `FIELD` e `INDEX`, además de campos e índices
  heredados mediante `LIKE` o `LIKE-SEQUENTIAL`. Tras `USE-INDEX` se
  ofrecen los índices de la tabla o buffer de la frase activa.
- **Inclusiones estáticas** anteriores al cursor, con variables,
  parámetros escalares, funciones y procedimientos internos. Por
  ejemplo, `RUN pIncl` ofrece `pIncluido`. Los símbolos de una rutina
  quedan en su ámbito.

El lector toma las inclusiones guardadas en disco. Busca en el orden del
PROPATH del proyecto y sigue inclusiones anidadas dentro de la carpeta.
Admite hasta 32 referencias, ocho niveles y 1 MiB acumulado, con 256 KiB
por archivo. La detección es léxica y admite inclusiones independientes
sin argumentos. La expansión de macros, las ramas condicionales y los
buffers dinámicos quedan fuera de este bloque.

Validado en Windows con OpenEdge 11.7 x86 y el perfil de prueba de dos
bases en solo lectura. GUI y TUI leyeron el catálogo real y completaron
`negocio.customer`, el campo `name` y el índice `cust-num`. Las tres
inserciones se revirtieron en un paso en cada interfaz. Las 12 huellas
SHA-256 de los archivos de base cotejados permanecieron iguales.

El ejemplo `progress/examples/schema-assistance` se copia al proyecto
sintético de prueba. Su `.cide/propath` contiene `primero` y `segundo` en
ese orden. Se verificaron 13 inserciones y su deshacer en cada interfaz.
OpenEdge 11.7 comprobó la sintaxis de `asistencia.p` con ese mismo PROPATH
y los aliases del perfil de prueba.

### Ir a definición en ABL

Dentro de un proyecto, coloca el cursor sobre el identificador y pulsa
**F12** o elige **Navegación → Ir a definición**. **Intro** abre la
declaración y **Ctrl+Intro** la abre al lado. El comando funciona en
GUI y TUI y admite declaraciones locales e inclusiones estáticas por
PROPATH, con la misma resolución que el autocompletado.

Se admiten buffers, tablas temporales, sus campos e índices declarados
en ABL, variables, parámetros escalares, funciones y procedimientos
internos. Para una llamada se pueden localizar rutinas internas
declaradas después del cursor. El análisis es léxico y trabaja con hasta
2 MiB de fuente. Para una inclusión se verifica la versión de su
contenido antes de abrirla. Los campos de una base conectada se
consultan en el catálogo de esquema.

Las pruebas dirigidas de GUI y TUI abrieron `primero/detalle.i` en la
declaración exacta usando el PROPATH del ejemplo. También cubrieron una
rutina declarada después de la llamada y el rechazo del salto a una
inclusión abierta con cambios sin guardar. OpenEdge 11.7 volvió a
comprobar la sintaxis del ejemplo.

### Clases importadas mediante USING

Pulsa **F12** sobre el nombre en `USING` o sobre el tipo usado en código
ABL. **Intro** abre la declaración y **Ctrl+Intro** la abre al lado.
GUI y TUI admiten importaciones explícitas y por comodín, incluidas las
declaradas en inclusiones estáticas. También se admiten tipos calificados
y usos en `AS`, `NEW`, `CAST`, `TYPE-OF`, `INHERITS` e `IMPLEMENTS`.

CIDE resuelve primero la importación explícita. Para comodines, sigue el
orden de las sentencias `USING`. Para cada tipo, consulta los directorios
en orden de PROPATH y verifica el nombre declarado en el `.cls`. Las
importaciones `FROM ASSEMBLY` quedan fuera de la navegación a fuentes
ABL. [Referencia USING](https://docs.progress.com/bundle/openedge-abl-reference-128/page/USING-statement.html)

OpenEdge 11.7 comprobó la sintaxis de los ejemplos con dos copias de
`paquete.Elemento` en PROPATH. GUI y TUI abrieron al lado la declaración
exacta de la primera copia. Los ejemplos también cubren la prioridad de
una importación explícita sobre un comodín y el comodín en una inclusión.

### Métodos y propiedades heredados en ABL

Pulsa **F12** sobre el miembro en `SUPER:Saludar("Ana")`,
`THIS-OBJECT:Titulo`, `objeto:Saludar(numero)` u `objeto:Titulo`.
**Intro** abre la declaración y **Ctrl+Intro** la abre al lado en GUI y
TUI. `SUPER` empieza en la clase base, `THIS-OBJECT` en la clase actual y
un receptor declarado empieza en su tipo estático. El recorrido sigue
`INHERITS`, `USING` y PROPATH hasta 16 clases base.
[Referencia SUPER](https://docs.progress.com/bundle/openedge-abl-reference-128/page/SUPER-system-reference.html)

La resolución de sobrecargas usa la aridad, el modo del argumento y los
tipos que puede identificar por literales o declaraciones simples. Prefiere
una firma exacta y admite ensanchamientos numéricos inequívocos de entrada.
Para `OUTPUT` e `INPUT-OUTPUT` reconoce tipos primitivos por coincidencia
exacta. Si se omite el modo y hay varias firmas posibles, muestra un aviso
de ambigüedad. [Modos y sobrecargas en ABL](https://docs.progress.com/bundle/abl-reference/page/Parameter-passing-syntax.html)

En `objeto:Obtener():Hijo:Titulo` se puede ir a la definición de `Titulo`
a través del tipo declarado que devuelve cada miembro. Cada tipo se resuelve
con los imports de la clase que declara el miembro. El análisis admite
hasta cuatro pasos intermedios dentro de una línea de código. Las llamadas
dinámicas y la inferencia de expresiones complejas siguen pendientes.

OpenEdge 11.7 compiló la jerarquía de tres clases y el programa
`consumidores/herencia.p`. GUI y TUI abrieron al lado las sobrecargas de
`Saludar` para texto y entero, incluida la llamada con una variable de
tipo `INTEGER`, la declaración de `Titulo` en la clase raíz, las cadenas
de propiedades y retornos y las sobrecargas por modo de `Ajustar`.
La GUI mostró el aviso de ambigüedad para `Saludar(?)` y para una llamada
a `Ajustar` sin modo explícito.

### Buscar referencias en ABL

Pulsa **Mayús+F12** o elige **Navegación → Buscar referencias** sobre una
declaración o un uso. La búsqueda recorre el documento activo y sus
inclusiones estáticas por PROPATH. Si el símbolo se declara en una inclusión
a nivel de programa, también busca en otros programas del proyecto que
resuelvan esa misma declaración. Puedes iniciar la búsqueda desde el
programa o desde el archivo incluido. Cada resultado muestra la línea.
Usa **Intro** para abrirlo o **Ctrl+Intro** para abrirlo al lado.

La búsqueda sigue la identidad de la declaración y distingue variables
homónimas de distintas rutinas, campos homónimos de tablas temporales
distintas y expansiones independientes de una inclusión. También reconoce
llamadas directas a rutinas internas. Para comparar programas, verifica
la ruta, el contenido y la posición de la declaración incluida. Examina
las versiones guardadas de hasta 512 archivos `.p`, `.w` y `.cls`, 128
directorios y 16 MiB de fuente e inclusiones. Muestra hasta 48 resultados
y avisa si la búsqueda queda parcial. **Llamado por** busca llamadas
externas estáticas a un programa.

Las pruebas locales cubrieron ámbitos homónimos, inclusiones repetidas,
el aviso de recorte y la apertura al lado en ambas interfaces. OpenEdge
11.7 comprobó la sintaxis del programa principal, de otro consumidor de
la inclusión anidada y de un programa con una declaración homónima
propia. GUI y TUI abrieron al lado el consumidor y descartaron el homónimo.

### Buscar referencias con XREF

Guarda el programa ABL, coloca el cursor sobre una tabla, campo o índice
de base de datos y elige **Proyecto → Buscar referencias con XREF** o
pulsa **Ctrl+Mayús+F12**. El comando usa el perfil Progress, sus aliases y
PROPATH. **Intro** abre la línea informada por el compilador y
**Ctrl+Intro** la abre al lado en GUI y TUI.

El AVM ejecuta `COMPILE XREF` en segundo plano. CIDE lee la salida para
reunir lecturas, escrituras, referencias de tabla y búsquedas por índice,
incluidas ubicaciones dentro de inclusiones. El AVM escribe XREF en un
directorio temporal y compila sin la opción `SAVE`. CIDE comprueba la
versión del archivo antes de abrir el resultado.
[Referencia COMPILE](https://docs.progress.com/bundle/openedge-abl-reference-128/page/COMPILE-statement.html)

La consulta admite hasta 32 programas, 128 archivos de fuente e
inclusiones, 16 MiB de fuente y 48 resultados. Indica si algún programa
quedó fuera por error o cuota. Si la línea contiene varios objetos
del mismo nombre, usa una línea que permita distinguirlos.
**Mayús+F12** sigue disponible para símbolos y llamadas ABL.

En OpenEdge 11.7, la GUI abrió al lado un acceso a `customer.Name`
generado por una macro en una inclusión. La TUI abrió al lado el mismo
campo en un programa con espacios en el nombre y recuperó referencias de
`customer` y `cust-num`. La búsqueda conservó los resultados válidos
cuando otro programa del ejemplo falló al compilar.

### Esquema del archivo y sus inclusiones

En la GUI, abre **Esquema → Símbolos**. En la TUI, elige **Proyecto →
Símbolos**. En un proyecto ABL, la lista reúne las declaraciones del
documento activo y de las inclusiones estáticas que resuelve por PROPATH.
Los símbolos de una inclusión muestran el nombre del archivo. Los
campos e índices de tablas temporales muestran el nombre de la tabla.

Selecciona una fila y pulsa **Intro** para ir a la declaración o
**Ctrl+Intro** para abrirla al lado. **F5**, con el panel enfocado, vuelve
a leer las inclusiones guardadas. La lista muestra hasta 63 entradas y
avisa si la recorta. El destino de una inclusión se verifica antes del salto.

Las pruebas de GUI y TUI mostraron declaraciones de `asistencia.p` y
de `primero/detalle.i`. Ambas abrieron al lado la posición exacta de
`pIncluido` y descartaron los símbolos de la copia homónima en el segundo
directorio de PROPATH. OpenEdge 11.7 comprobó la sintaxis del ejemplo con
ese orden de búsqueda.

## Validación local — 2026-11-11

OpenEdge 11.7 x86 en Windows. Se probó el orden de dos directorios con una
inclusión homónima, una inclusión anidada, rutas con espacios, un diagnóstico
navegable por inclusión ausente, la generación de `.r`, la ejecución por
lotes, un error de ejecución y la cancelación de una AVM en curso. El flujo
se ejercitó sin bases conectadas. Pasaron las tres pruebas con AVM real
(adaptador compartido, GUI y TUI), 21 pruebas ordinarias de `jc-progress`,
15 de interacción de `jc-app`, tres pruebas de menú nativo, formato y Clippy.
La entrega optimizada `target/m3-progress-next/release` pasó 14 comprobaciones
del panel inferior de la GUI en ventana oculta. En una terminal privada,
la TUI de esa entrega ejecutó el procedimiento desde Herramientas, mostró
su texto en Salida y abrió Problemas al compilar con una inclusión ausente.

La prueba real compartida genera su propio proyecto de prueba. Define
`CIDE_TEST_PROGRESS_EXE` con la ruta al AVM y `CIDE_TEST_PROGRESS_EXPECT_VERSION`
con la versión que quieres confirmar. Ejecuta:

```console
cargo test -p jc-progress --features source-services --lib progress_real_propath --locked --offline -- --ignored
```

Para los recorridos de interfaz, define `CIDE_TEST_PROGRESS_PROJECT` con la
raíz de un proyecto sintético con el perfil anterior, `valido.p` que imprime
`CIDE-M3` y `invalido.p` que incluye `{ausente.i}`:

```console
cargo test -p jc-gui -p jc-tui --lib command_routes_to_output_and_problems --locked --offline -- --ignored
```

En el segundo bloque se usó `prodb` para crear dos bases aisladas a partir
de `demo` y `empty` de OpenEdge 11.7. Un perfil `.pf` con `-1 -RO`
conectó ambas con nombres lógicos distintos. Se compiló y ejecutó un
procedimiento que leyó `negocio.customer` a través del alias. También se
verificaron ambas interfaces, la base vacía, un alias inválido y la
retención del catálogo anterior. Para repetirlo, prepara esas copias con
los nombres lógicos y aliases del ejemplo, guarda el archivo
`.cide/desarrollo.local.pf` con rutas absolutas y define
`CIDE_TEST_PROGRESS_SCHEMA_PROJECT` con la carpeta del proyecto de prueba.

```console
cargo test -p jc-progress -p jc-gui -p jc-tui --features jc-progress/source-services --lib progress_real_schema --locked --offline -- --ignored
```

Pasaron los tres recorridos con esquema real, el caso sin aliases, las
consultas `JCIDE/1` y `JCIDE/2` y las pruebas de regresión del puente.
La entrega `target/m3-progress-schema-final/release` pasó 14 comprobaciones
del panel inferior de la GUI oculta. La TUI optimizada actualizó el catálogo
en una terminal privada, mostró el alias inválido en Problemas y reabrió
el último catálogo válido desde su encabezado. Se verificaron los hashes
de los diez archivos de datos y control de las bases de prueba.

## Compatibilidad con OpenEdge 10.0B y perfil del ERP

Los auxiliares se compilan como fuente procedural con el AVM configurado.
Los `.r` se generan para ese mismo AVM. Para la prueba en 10.0B, usa su
`_progres.exe`, configura `version=10.0B` y prepara bases de desarrollo
compatibles con esa versión. El recorrido de 10.0B cubre `.p` y `.w`. Las
clases ABL se prueban con una versión que admita ese lenguaje. [Compatibilidad del código r](https://progress.my.salesforce-sites.com/ProgressKB/articles/Article/20922) · [Historial de versiones de Progress](https://community-archive.progress.com/forums/00026/08003.html)

Para validar un perfil del ERP, abre la raíz que contenga `.cide/progress.local.ini`
con un `.pf` de desarrollo autorizado para consultar el diccionario en solo lectura.
Define `CIDE_TEST_PROGRESS_ERP_PROJECT` con esa raíz y ejecuta:

```console
cargo test -p jc-gui -p jc-tui --lib progress_real_erp_ --locked --offline -- --ignored --test-threads=1
```

Los recorridos actualizan el esquema, comprueban que **Salida** identifique
el AVM y muestre tablas, y vuelven a abrir el catálogo desde la caché. El
auxiliar consulta metadatos del diccionario con `NO-LOCK`. El perfil y sus
credenciales deben quedar en archivos locales del entorno de prueba.

En este equipo está instalada la AVM 11.7. Pasaron seis recorridos con esa
AVM, dos por cada capa de prueba: adaptador, GUI y TUI. Se comprobó el rechazo
de una versión configurada incompatible antes de ejecutar el procedimiento,
y las dos versiones anteriores del protocolo conservaron sus respuestas.
Las bases de muestra conservaron sus hashes. También pasaron 22 pruebas
ordinarias de `jc-progress`, formato y Clippy del workspace. La ejecución en
10.0B y la consulta de un perfil real del ERP quedan pendientes de esos entornos.

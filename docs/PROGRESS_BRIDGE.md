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
`CIDE_TEST_PROGRESS_EXE` con la ruta al AVM y ejecuta:

```console
cargo test -p jc-progress --features source-services --lib progress_real_ --locked --offline -- --ignored
```

Para los recorridos de interfaz, define `CIDE_TEST_PROGRESS_PROJECT` con la
raíz de un proyecto sintético con el perfil anterior, `valido.p` que imprime
`CIDE-M3` y `invalido.p` que incluye `{ausente.i}`:

```console
cargo test -p jc-gui -p jc-tui --lib progress_real_ --locked --offline -- --ignored
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

Queda por verificar OpenEdge 10.0B y aplicar el perfil a una base del ERP.

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
```

Las rutas de `executable`, `pf` e `ini` pueden ser absolutas o relativas a
la raíz. `pf` e `ini` son opcionales y se pasan explícitamente al AVM.
Configura aquí solo ejecutables y parámetros de un entorno de desarrollo de
confianza. La sesión por lotes puede acceder a los recursos que declaren el
programa y su `.pf`. Las páginas de códigos verificadas en este bloque son
ISO-8859-1 para diagnósticos y IBM850 para salida.

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
- **Cancelar tarea ABL** solicita terminar el proceso AVM propio. El trabajo también vence a los 30 segundos y se invalida si cambia la revisión del documento. Una cancelación no revierte efectos que el procedimiento ya haya producido.

Los errores y advertencias del compilador aparecen en **Problemas**. Cuando
el AVM identifica una línea del documento compilado, la fila permite ir al
inicio de esa línea y muestra la columna informada. Los diagnósticos de
inclusiones u otras fuentes se muestran con la ubicación que entrega el AVM.
Solo se inicia una tarea ABL a la vez. Los resultados y la captura de texto
están acotados. El proceso se ejecuta con la raíz del proyecto como carpeta
de trabajo.

## Protocolo

La primera versión usa un archivo ASCII de solicitud y una respuesta ASCII con texto codificado en hexadecimal. Así los mensajes pueden conservar bytes de la sesión sin afirmar que son UTF-8.

Registros de respuesta: `DIAG`, `TABLE`, `FIELD`, `INDEX`, `INDEX_FIELD`, `END|OK` y `END|ERROR`.

El parser Rust impone límites de tamaño y rechaza respuestas incompletas o contradictorias.

## Esquema

El helper consulta `_File`, `_Field`, `_Index` y `_Index-Field` mediante `DICTDB`. El catálogo resultante es para navegación/autocompletado; no sustituye una base de esquema cuando el compilador necesita resolver referencias.

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

Queda por verificar OpenEdge 10.0B y un perfil de desarrollo con bases,
aliases y parámetros de arranque reales. La actualización de esquema todavía
no está conectada a las interfaces.

# Instalador de CIDE para Windows x64

El MSI instala CIDE en `Archivos de programa\CIDE`, requiere permisos de administrador y agrega **CIDE** y **CIDE Terminal** al menú Inicio. Incluye la CLI, los nombres de ejecutable de compatibilidad y los avisos de terceros. La recuperación y la configuración del usuario quedan fuera de los archivos administrados por el MSI.

## Construcción local

Usar Python 3.11 o posterior, WiX 6.0.2 y `WixToolset.UI.wixext` 6.0.2.
Ejecutar desde la raíz del proyecto con `work/packages` creado y rutas de
salida nuevas. Para el MSI, usar una consola de desarrollo x64 o pasar
`--rc` y `--link` con las rutas al compilador de recursos del SDK de
Windows y al enlazador de MSVC.

```powershell
python -B scripts/build-cide-local.py --output target/installer-next/release
python -B scripts/package-cide.py --bin-dir target/installer-next/release --output work/packages/CIDE-input.zip
python -B scripts/build-cide-installer.py --package work/packages/CIDE-input.zip --wix $wixExe --ui-extension $wixUiDll --output work/packages/CIDE-0.1.0-windows-x64.msi
& $wixExe msi validate work/packages/CIDE-0.1.0-windows-x64.msi
```

Para preparar una actualización, usa `--msi-version 0.1.1` y un nombre
de salida que coincida con esa versión. Windows Installer compara los
tres campos numéricos del MSI mediante el mismo código de actualización.
La versión de transporte declarada por el ZIP se valida por separado.

Para volver a emitir el mismo número de versión, pasa
`--allow-same-version-upgrade` al construir el MSI. Este paquete podrá
reemplazar una instalación anterior con el mismo número. Windows Installer
compara las recompilaciones que usan ese número como versiones equivalentes.

Para comprobar el contenido, usar `msiexec /a` con `TARGETDIR` apuntando a una carpeta de pruebas. Comparar la extracción con el manifiesto del ZIP, aplicando la selección de archivos de `build-cide-installer.py`. El MSI actual se construye sin firma digital.

## OmniCore en la GUI

Empaqueta el componente con `--omnicore-dir` en `package-cide.py`.
El directorio indicado debe contener una compilación autónoma de OmniCore
para `win-x64`, con `observe.changedFiles.v1` y `ask.stdin.v1`. Copia su
licencia y avisos de terceros a `licenses/OmniCore-LICENSE.txt` y
`licenses/OmniCore-THIRD-PARTY-NOTICES.md`. Copia los del runtime de .NET
a `licenses/dotnet-LICENSE.txt` y `licenses/dotnet-THIRD-PARTY-NOTICES.txt`.
El MSI instala esos archivos bajo `bin/omnicore`. CIDE encuentra ahí
`omni.exe` al abrir el panel, y el runtime viaja con el componente.

Valida la conexión desde un editor vacío con
`python -B scripts/check-omnicore-panel.py --omnicore <ruta-a-omni.exe>`.
La prueba usa únicamente una carpeta de prueba y un proveedor simulado
en `127.0.0.1`.

El MSI local 0.1.4 se generó en Windows x64 con WiX 6.0.2 y OmniCore
autónomo con .NET 10.0.14. Pasaron la comprobación de formato, nueve pruebas
dirigidas de la GUI, Clippy para `jc-gui` y la compilación optimizada local.
`wix msi validate` terminó con código 0 y una advertencia ICE60 de idioma
en metadatos de un archivo. `msiexec /a` extrajo 1.053 archivos y sus SHA-256
coincidieron con el manifiesto, incluidos 387 de OmniCore. La prueba con
`check-omnicore-panel.py --bundled` encontró el componente extraído sin
una ruta configurada, envió una consulta desde un editor vacío y recibió
la respuesta local. Queda pendiente instalar y probar el paquete en el
segundo equipo.

## Iconos del menú Inicio y aplicaciones instaladas

Desde el MSI 0.1.3, **CIDE** usa el icono naranja y **CIDE Terminal** usa
el icono original. El instalador publica cada icono desde su propio
archivo PE de recursos. **Aplicaciones instaladas** también usa el icono
naranja. Para comprobar el paquete, las filas `CideGuiShortcut` y
`CideTuiShortcut` de la tabla `Shortcut` deben apuntar a
`CideGuiIcon.exe` y `CideTuiIcon.exe` de la tabla `Icon`, respectivamente.

Al ejecutar el MSI 0.1.3 sobre 0.1.2, Windows Installer reemplaza los
accesos directos como parte de la actualización. Si había un acceso
anclado que sigue mostrando el icono anterior, vuelve a anclarlo desde
el menú Inicio después de actualizar.

El MSI local `work/packages/CIDE-0.1.3-windows-x64.msi` pasó WiX ICE
y la extracción administrativa de sus 666 archivos. Se verificaron
las dos asociaciones de `Shortcut`, el icono de Aplicaciones instaladas
y la carga de ambos iconos del MSI con la API del Explorador de Windows.

## Validación de v0.1.0-preview.1

El 11 de noviembre de 2026, Windows Installer devolvió `0` en los cuatro pasos con permisos de administrador: instalación, desinstalación, reinstalación y desinstalación final. Se verificaron el registro del producto, los archivos instalados y los destinos de ambos accesos del menú Inicio. La prueba abrió la GUI en una ventana oculta y la TUI en una pseudoconsola privada desde `Archivos de programa`.

Ambas interfaces cerraron normalmente con cambios sin guardar. El depósito de recuperación, situado en un `LOCALAPPDATA` de prueba, conservó sus archivos byte por byte después de desinstalar y reinstalar. Ambas interfaces restauraron los cambios y el historial de edición. Los documentos originales conservaron sus bytes. La última desinstalación retiró el registro, los accesos y la carpeta de instalación. Los registros locales quedaron en `work/packages/cide-msi-lifecycle-19c54cc7`.

## Validación local de 0.1.1

WiX validó las tablas ICE del MSI de M5 en Windows. La extracción
administrativa mediante `msiexec /a` verificó los 666 archivos contra
el manifiesto del ZIP. Los ejecutables de esa extracción restauraron
cambios sin guardar en tres procesos GUI ocultos y tres procesos TUI en
una terminal privada. Los originales de las pruebas conservaron sus
bytes. La comparación Git en Código pasó desde la TUI extraída.
El paquete local es `work/packages/CIDE-0.1.1-m5-windows-x64.msi`.

El ciclo de instalación por máquina de 0.1.1, actualización desde 0.1.0
y desinstalación requiere una sesión elevada de Windows. La extracción
administrativa y las pruebas de recuperación usaron carpetas de prueba.

## Validación local de 0.1.2

Las matrices locales de M5 pasaron formato, pruebas, Clippy, compilación
optimizada y demo. Windows x64 registró 1.032 pruebas aprobadas y 51
marcadas para recorridos aparte. Debian 13 en WSL2 registró 714 aprobadas
y 28 marcadas para recorridos aparte. Se revalidaron las cuatro pruebas
de recuperación de TUI en Windows tras ajustar su auxiliar de prueba.

El MSI `work/packages/CIDE-0.1.2-m5-windows-x64.msi` pasó ICE con WiX
6.0.2. Windows Installer extrajo 666 archivos que coincidieron por
longitud y SHA-256 con el manifiesto. La prueba abrió una GUI oculta y
una TUI en una pseudoconsola privada. Ambas recuperaron cambios sin
guardar desde 0.1.1 y conservaron el historial en una segunda reapertura
con 0.1.2. Los documentos originales quedaron intactos.

El 18 de noviembre de 2026 pasó el ciclo real por máquina en segundo plano.
Windows Installer instaló 0.1.1, la sustituyó por 0.1.2, desinstaló 0.1.2,
la reinstaló y la retiró al terminar. La GUI verificó el texto sin guardar,
el cursor y el historial. La TUI recuperó la edición y comprobó Deshacer
y Rehacer. Ambas recuperaron la sesión después de actualizar y después
de reinstalar. Los originales conservaron sus bytes y se retiraron
los dos registros de producto, los binarios y los accesos de prueba.

El ejecutor elevado devolvió código 0. La evidencia accesible desde una
sesión ordinaria está en
`work/m5-release-ng50jw25/msi-machine-verification.json`, con los cinco
eventos correspondientes de Windows Installer y el inventario final.

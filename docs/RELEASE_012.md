# CIDE 0.1.12 — entrega local y validación Windows

## Cambios

- Preferencias de apariencia con tarjetas de tema, controles gráficos y
  ajustes combinados; Aplicar confirma el conjunto y Cancelar lo restaura.
- Iconos vectoriales de carpetas y archivos con cubiertas, pliegues y sombras,
  color por extensión y escala DPI en Archivos, Esquema y OmniCore.
- Vínculos de clases ABL en propiedades, retornos METHOD, NEW y USING.
- Inclusiones e importaciones de fuentes locales en GUI y TUI, excluyendo
  bibliotecas del sistema. Se conserva PROPATH y la resolución compartida.
- Corrección del salto doble al abrir un archivo y ubicar su declaración.
  Un Ctrl+izquierda vuelve al punto de origen.

El alcance y los límites léxicos están en
[SOURCE_TYPE_AND_FILE_LINKS.md](SOURCE_TYPE_AND_FILE_LINKS.md) y el diseño en
[GUI_VISUAL_CONTROLS.md](GUI_VISUAL_CONTROLS.md).

## Comprobaciones locales

Windows x64, Rust 1.88 fijado, cachés existentes, sin GitHub Actions.
`python scripts/check-local.py --steps fmt` pasó. También pasaron Clippy
all-targets de jc-languages, jc-app, jc-progress, jc-gui y jc-tui, y la
compilación optimizada offline del workspace con CIDE_RELEASE_VERSION=0.1.12.

Las comprobaciones dirigidas cerraron 14 casos del controlador, 9 GUI y
16 TUI; quedaron omitidos 8 casos GUI y 9 TUI que requieren otros entornos.
Las matrices nuevas prueban once rutas por frontend y el regreso al byte
original. El caché léxico pasó seis casos y el catálogo resolvió clases de
propiedades, retornos y NEW. Los casos de rutas estáticas descartan comentarios,
rutas calculadas y paquetes del sistema.

Computer Use probó en la GUI real el doble clic y Ctrl+Intro de clases e
inclusiones, además del regreso/avance del historial. La instancia debug
salió normalmente y sus cuatro fuentes permanecieron intactos. También se
habían comprobado físicamente doble clic y Ctrl+Intro de métodos en los siete
lenguajes. Ctrl+clic físico sigue pendiente por la restricción de integridad
del Teclado en pantalla; no se presenta como aprobado por compartir controlador.
La validación de esta entrega no constituye una ejecución nueva en Linux.

## MSI

Fuente de compilación: `26568edf1578859a9d2f5c66146dcae30a5dfd99`.
Archivo: `CIDE-0.1.12-windows-x64.msi`.
Tamaño: 66.636.744 bytes.
SHA-256: `94f334a67cc8d8769005128120af94f007d23fe9cb2391008ea9a12fdfb42a9c`.

WiX validó el MSI y conservó la advertencia ICE60 de metadatos de idioma.
La extracción administrativa coincidió con los 1.057 archivos esperados por
tamaño y SHA-256. Los binarios extraídos pasaron 15 comprobaciones GUI de
navegación, 17 del shell, 16 de preferencias y siete de TUI mediante ConPTY,
con fuentes y metadatos de prueba conservados. La extracción administrativa
no se presenta como una instalación y desinstalación completa nueva.

El instalador se distribuye sin firma Authenticode. La comprobación adicional
de Acerca de en la GUI extraída encontró de nuevo captura negra y fallo de
activación de Windows; la instancia propia espera un escritorio accesible
para observar su versión y cerrarla normalmente. Esto no invalida las
comprobaciones ocultas ni la prueba física anterior sobre el mismo código.
Los registros están bajo work/source-links-validation, work/desktop-source-links
y work/packages/cide-012-visual; las capturas nativas pertenecen a ventanas
propias, no a una aceptación física de toda la plataforma.

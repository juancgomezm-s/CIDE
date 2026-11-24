# CIDE 0.1.14 — minimapa, temas y fuentes

[Descargar el MSI para Windows x64](https://github.com/juancgomezm-s/CIDE/releases/download/v0.1.14/CIDE-0.1.14-windows-x64.msi).
Incluye GUI, TUI, CLI y el runtime autónomo de OmniCore.

Fuente de compilación: `f8d5f9a646fd401ace3319bc7dc3d54372d8d6ef`.
«Acerca de» incorpora la versión 0.1.14 mediante el entorno de compilación.

## Cambios incluidos

- El minimapa usa los colores sintácticos del editor y cobertura obtenida de
  la fuente elegida. Cada panel mantiene su propio archivo. En documentos
  residentes conserva el contexto de comentarios y cadenas multilínea.
- Los vínculos se subrayan sin sustituir el color del token: las inclusiones
  ABL permanecen magenta tanto en el editor como en el minimapa.
- Apariencia permite elegir una fuente monoespaciada instalada mediante el
  selector de Windows, con vista previa, Aplicar/Cancelar y conservación en la sesión.
- Nuevos temas Claro nítido, Papel cálido, Grafito profundo y Eclipse oscuro;
  tonos del tema claro revisados y literales numéricos diferenciados.
- Inclusiones de Esquema en formato Declaraciones: una ruta por línea, sin
  subcarpetas y conservando la apertura normal y al lado.
- Esquema sin el recorte de 63 entradas, con progreso del análisis. Relaciones
  estáticas de hasta tres niveles en «Llama a» y uno en «Llamado por», con
  detección de ciclos y destinos no resueltos explícitos.

Detalles y límites: [apariencia e inclusiones](APPEARANCE_FONTS_FLAT_INCLUDES.md),
[fidelidad del minimapa](MINIMAP_SOURCE_FIDELITY.md) y
[Esquema y relaciones](OUTLINE_COMPLETE_RELATIONS.md).

## Verificación local

Windows x64, Rust 1.88 fijado, dependencias offline y cachés existentes.
La compilación optimizada del workspace terminó sin errores.

Antes de empaquetar se revisó directamente en el escritorio la GUI del mismo
código: `principal.p` y `calculos.i` en paneles distintos; temas CIDE,
Claro nítido y Eclipse oscuro; fuentes Consolas y Courier New; cambio de
pestaña y actualización del minimapa. Este recorrido utilizó el ejecutable
debug de `target/minimap-fidelity-next`, sin editar ni guardar los fixtures.

El MSI optimizado pasó WiX/ICE. Se verificaron ProductVersion 0.1.14, fabricante,
UpgradeCode conservado y ProductCode distinto de 0.1.13. Se mantiene una
advertencia ICE60 de idioma ya presente en la entrega anterior; no hubo errores
ni se habilitó la actualización entre paquetes con el mismo número de versión.

Los **1.057 archivos** de la extracción administrativa coincidieron por tamaño
y SHA-256 con el contenido esperado. Los ejecutables extraídos pasaron:

- 13 comprobaciones de sintaxis GUI con dibujo GDI en ventana propia oculta.
- 17 comprobaciones del shell GUI.
- 16 comprobaciones de selectores y preferencias GUI.
- Siete comprobaciones TUI mediante ConPTY propio.
- Recursos de iconos de los seis ejecutables, con siete tamaños de 16 a 256 px.

Los archivos de prueba y los metadatos Git permanecieron intactos. Los
recorridos ocultos del MSI se distinguen de la revisión anterior de escritorio.
No se repitieron una instalación/desinstalación completa, Linux, la suite
global ni consultas reales a OmniCore. No se ejecutaron GitHub Actions.
El inventario offline conserva las dos ausencias de avisos GNU ya documentadas
en 0.1.13; no se agregaron dependencias.

## Archivo

- MSI: `work/packages/cide-014/CIDE-0.1.14-windows-x64.msi`.
- Tamaño: **66.841.544 bytes**.
- SHA-256: `d0b87fc5275d2f717e988e0c063d8d24e79fa10a7aefcc435cc0e714c0b38954`.
- Requiere permisos de administrador y se distribuye **sin firma Authenticode**.
- Registros: `work/packages/cide-014/{build-release,validation,publication}.log`
  y `resultado.json`; revisión de escritorio en
  `work/minimap-fidelity/desktop-validation.json`.

El minimapa sigue siendo una reducción acotada. No se acredita contexto
léxico global de comentarios o cadenas para archivos paginados de varios GB.
Se preservaron entregas, sesiones y ventanas anteriores.

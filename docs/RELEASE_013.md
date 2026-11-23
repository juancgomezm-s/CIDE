# CIDE 0.1.13 — entrega final del bloque OmniCore

Fuente de compilación: `5f76f7408ff416455707c1185bd29048a17d1b00`.
Incluye los iconos semánticos de Esquema, Ajustes de OmniCore y los cambios de
conversación descritos en [OMNICORE_EDITOR_INTEGRATION.md](OMNICORE_EDITOR_INTEGRATION.md).

En OmniCore → Ajustes, **Mostrar herramientas y razonamiento** empieza
desactivado. La actividad compartida por el runtime se separa de la respuesta;
activar la opción no cambia el mensaje ni la propuesta de código. La preferencia
se conserva en los metadatos de recuperación GUI v10. No se inventa ni expone
razonamiento privado del proveedor.

## Prueba directa en la GUI

La validación con computer-use completó los recorridos solicitados:

- Ajustes comienza con Mostrar herramientas y razonamiento desmarcado. Al
  marcarlo se vieron filesystem.write/read/patch; al desmarcarlo desapareció la
  actividad, conservando las respuestas.
- Consulta devolvió una función Sumar en un bloque ABL. Insertar abrió la
  comparación, Aplicar insertó el código y un solo Deshacer vació el documento.
- Editar creó suma-gui.p desde el composer y CIDE lo abrió como código normal.
- Otra solicitud editó su RETURN y abrió automáticamente el diff rojo/verde.
  Se corrigieron la ruta del modelo no vinculada y el documento desactualizado
  al cerrar la comparación. La repetición cambió * 2 por * 3; el diff mostró
  ambos valores y, al cerrarlo con la ×, el editor mostró el * 3 actualizado.

Las consultas y los efectos se realizaron desde CIDE con gpt-6-sol. No se
crearon archivos mediante un script para simular el trabajo del modelo. El
detalle y los límites están en OMNICORE_EDITOR_INTEGRATION.md. La última prueba
conservó los ajustes de apariencia elegidos por Juan; después se dejó de tomar
el escritorio.

## Comprobaciones locales

Windows x64, Rust 1.88 fijado y cachés existentes. Pasaron fmt, 46 pruebas
dirigidas OmniCore (dos casos especiales omitidos), diez de recuperación GUI,
una del Markdown de mensajes y Clippy app/GUI all-targets. La compilación
optimizada offline del workspace y el adaptador .NET 10 terminaron sin errores.
No se ejecutaron GitHub Actions ni pruebas nuevas de Linux.

Después de las correcciones pasaron tres pruebas nuevas de actualización de
archivos (identidad y guardado, cambios locales con Deshacer, raíz y tamaño),
Clippy app/GUI all-targets y la compilación debug. El comprobador reproducible
scripts/check-omnicore-edit.py pasó cuatro casos del Host real sin proveedor:
Local e IncludedQuota vinculados; MeteredCurrency y modelo inexistente
rechazados antes del Run, sin modificar configuración global ni archivos del
workspace. Estas pruebas se distinguen de las consultas reales desde la GUI.

## MSI

- Archivo local: `work/packages/cide-013-final/CIDE-0.1.13-windows-x64.msi`.
- Tamaño: 66.726.856 bytes.
- SHA-256: `2c60659c55381d060a3607d76eab2797fe75f6cc0a4a48362931e7db0455cf8e`.
- Sin firma Authenticode.
- [Entrega en GitHub y descarga del MSI](https://github.com/juancgomezm-s/CIDE/releases/tag/v0.1.13).

WiX validó el MSI con el acceso normal del usuario. Conservó ICE60 de idioma y
ICE61 porque se habilitó expresamente la actualización desde el candidato con
la misma versión 0.1.13. La extracción administrativa coincidió por
tamaño y SHA-256 con los 1.057 archivos esperados. No equivale a una instalación
y desinstalación completa nueva.

Los ejecutables extraídos pasaron 17 comprobaciones del shell GUI, 16 de
selectores/preferencias y siete de TUI con ConPTY. Sus fixtures permanecieron
intactos. Navegación no se repitió porque no cambió en este bloque. El manifiesto
conserva las dos ausencias de avisos GNU del inventario offline que ya figuraban
en 0.1.12; no se añadieron dependencias. Las licencias de gitoxide y OmniCore
permanecen en el paquete.

Registros: `work/semantic-outline-icons/integration-*.log`,
`work/semantic-outline-icons/refresh-*.log`,
`work/omnicore-edit-check/`, `work/packages/cide-013-final/resultado.json` y
`work/packages/cide-013-final/validation.log`. El candidato anterior permanece
en su ubicación original; el archivo de esta entrega está en cide-013-final.

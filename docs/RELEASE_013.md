# CIDE 0.1.13 — candidato local de integración OmniCore

Fuente de compilación: `880a780a28b79be7a5caac60e89aca5343cc2874`.
Incluye los iconos semánticos de Esquema, Ajustes de OmniCore y los cambios de
conversación descritos en [OMNICORE_EDITOR_INTEGRATION.md](OMNICORE_EDITOR_INTEGRATION.md).

En OmniCore → Ajustes, **Mostrar herramientas y razonamiento** empieza
desactivado. La actividad compartida por el runtime se separa de la respuesta;
activar la opción no cambia el mensaje ni la propuesta de código. La preferencia
se conserva en los metadatos de recuperación GUI v10. No se inventa ni expone
razonamiento privado del proveedor.

## Comprobaciones locales

Windows x64, Rust 1.88 fijado y cachés existentes. Pasaron fmt, 46 pruebas
dirigidas OmniCore (dos casos especiales omitidos), diez de recuperación GUI,
una del Markdown de mensajes y Clippy app/GUI all-targets. La compilación
optimizada offline del workspace y el adaptador .NET 10 terminaron sin errores.
No se ejecutaron GitHub Actions ni pruebas nuevas de Linux.

Desde la GUI real se comprobó Ajustes, el cambio gpt-6-luna → gpt-6-sol,
dos consultas reales y Actualizar sin perder el chat. La corrección posterior de
Editar, Insertar/Aplicar, las tarjetas nuevas y el interruptor no se presentan
como aprobados desde el escritorio: la captura volvió a quedar negra y Windows
negó GetCursorPos. La admisión nativa ScopedCoder se comprobó sin invocar un
proveedor; no sustituye una escritura realizada desde el chat. La instancia propia
anterior conserva pendiente su cierre normal hasta que el escritorio sea accesible.

## MSI

- Archivo local: `work/packages/CIDE-0.1.13-windows-x64.msi`.
- Tamaño: 66.702.280 bytes.
- SHA-256: `b56bd8fdffafa2611ff7ad2307de1555349b44b65dcc03cc0b7064c1a447bb9a`.
- Sin firma Authenticode; candidato local, no publicado en GitHub.

La validación ICE restringida no pudo acceder al servicio Windows Installer.
Con el acceso normal del usuario, WiX validó el mismo MSI y conservó únicamente
la advertencia ICE60 de idioma. La extracción administrativa coincidió por
tamaño y SHA-256 con los 1.057 archivos esperados. No equivale a una instalación
y desinstalación completa nueva.

Los ejecutables extraídos pasaron 17 comprobaciones del shell GUI, 16 de
selectores/preferencias y siete de TUI con ConPTY. Sus fixtures permanecieron
intactos. Navegación no se repitió porque no cambió en este bloque. El manifiesto
conserva las dos ausencias de avisos GNU del inventario offline que ya figuraban
en 0.1.12; no se añadieron dependencias. Las licencias de gitoxide y OmniCore
permanecen en el paquete.

Registros: `work/semantic-outline-icons/integration-*.log`,
`work/packages/cide-013-omnicore/resultado.json` y
`work/packages/cide-013-omnicore/validation-normal.log`.

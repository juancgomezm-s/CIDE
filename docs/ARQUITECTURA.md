# Arquitectura

JC-IDE separa cuatro responsabilidades: almacenamiento de documentos, modelo de aplicación, proveedores de lenguaje y presentación.

## Capas

- **jc-core**: documento byte-preserving, piece table, revisiones, undo/redo acotado, compactación y regiones.
- **jc-text**: interpretación explícita de encoding, line endings y coordenadas sin poseer los bytes canónicos.
- **jc-files**: ciclo de vida de archivo, conflictos externos, creación segura y reemplazo atómico.
- **jc-app**: comandos, keymap contextual, foco, menús, pestañas, paneles y diálogos. No dibuja nada.
- **jc-languages**: perfiles, detección y metadatos declarativos de capacidades (por ejemplo, comentarios). No inicia servicios por reconocer una extensión.
- **jc-progress**: integración explícita con una instalación real de OpenEdge; candidata a adaptador de la extensión interna `progress.abl`.
- **jc-tui / jc-gui**: frontends futuros que consumen el mismo jc-app y jc-core.

La separación entre núcleo universal, capacidades por lenguaje y extensiones internas está definida en [EXTENSION_ARCHITECTURE.md](EXTENSION_ARCHITECTURE.md).

La regla central es que una acción de negocio existe una sola vez como `CommandId`. Un menú, una tecla, la command palette o un botón gráfico resuelven el mismo comando. Los frontends no mantienen atajos duplicados ni implementaciones paralelas de guardar, compilar, buscar o navegar pestañas.

## TUI y GUI

La TUI se implementará primero como consumidor del modelo común porque fuerza a definir correctamente teclado, foco y estados. La GUI utilizará el mismo registro de comandos y las mismas condiciones de disponibilidad.

El contrato normativo de la TUI está en [TUI_INTERACTION.md](TUI_INTERACTION.md).

## Codificación y coordenadas

El documento canónico contiene bytes. La página de códigos es metadato explícito. `jc-text` puede interpretar UTF-8, Windows-1252, ISO-8859-1 e IBM850, pero no convierte el almacenamiento. Un legacy ambiguo exige selección de proyecto/documento. El núcleo no normaliza CRLF, BOM ni salto final.

Se distinguen offsets de byte, caracteres decodificados, grafemas, columnas visuales, posiciones LSP y posiciones reportadas por compiladores. Las conversiones ocurren en límites explícitos.

## Ligereza

Abrir y editar no requiere Progress, Git, LSP, terminal ni agentes. Los servicios se activan bajo demanda y su consumo se mide junto con el proceso principal.

## Interfaz Windows

La tecnología gráfica sigue pendiente de medición. La decisión entre un renderer/control propio y un componente existente como Scintilla debe considerar memoria, arranque, IME, accesibilidad y archivos grandes, no únicamente el lenguaje de implementación.

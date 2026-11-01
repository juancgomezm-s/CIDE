# Arquitectura

JC-IDE separa deliberadamente el editor, los lenguajes y los proveedores externos.

## Capas

- **jc-core**: documento byte-preserving, piece table, revisiones, undo/redo y regiones.
- **jc-languages**: perfiles y detección. No ejecuta LSP ni compiladores.
- **jc-progress**: contrato explícito para usar una instalación real de OpenEdge/ABL.
- **UI**: pendiente. Debe permanecer fuera del núcleo y cargar servicios bajo demanda.

## Regla de codificación

El archivo en disco es una secuencia de bytes. La página de códigos es metadato del documento/proyecto. El renderizador y los servicios convierten de forma explícita en sus límites; el núcleo no normaliza UTF-8, CRLF, BOM ni salto final.

## Coordenadas

Se distinguen offsets de byte, caracteres Unicode, grafemas, posiciones LSP UTF-16, columnas visuales y posiciones reportadas por compiladores. No son intercambiables.

## Ligereza

El editor debe poder abrir y editar sin iniciar Progress, Git, LSP o agentes. Los servicios pesados se activan bajo demanda y se mide el consumo total, no solo el proceso de UI.

## Interfaz Windows

Pendiente de decisión entre renderizado nativo/propio y componentes existentes como Scintilla. Cualquier elección deberá medirse en memoria, arranque, IME, accesibilidad y archivos grandes.

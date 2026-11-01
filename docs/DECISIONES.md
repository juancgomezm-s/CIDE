# Registro de decisiones

## D01 — Núcleo de edición independiente

**Aceptada.** Rust se usa para el núcleo inicial. Documentos y operaciones no dependen de widgets ni de una sesión de Progress.

## D02 — Bytes como representación canónica

**Aceptada.** Los fuentes no se pueden asumir UTF-8. El almacenamiento conserva bytes y las conversiones textuales son explícitas.

## D03 — Reutilización selectiva de Lime

**Aceptada.** Solo se portan comportamientos pequeños, probables y atribuibles. No se porta el editor completo ni su compatibilidad Python/Sublime.

## D04 — Progress como proveedor de compilación

**Aceptada.** La instalación seleccionada por proyecto es la autoridad para sintaxis, compilación y resolución estática de esquema.

## D05 — Catálogo local de esquema

**Aceptada.** La navegación y el completado consultarán metadatos locales actualizables sin mantener obligatoriamente una conexión permanente.

## D06 — Servicios opcionales

**Aceptada.** Progress, Git, LSP, terminal y agentes se inician bajo demanda y no forman parte del consumo basal.

## D07 — Interfaz Windows

**Pendiente.** Se medirá interfaz/renderizado propio frente a componentes existentes antes de comprometer la arquitectura gráfica.

## D08 — Integración con OmniCore

**Diferida.** OmniCore conserva su núcleo C# y contratos propios. JC-IDE podrá intercambiar revisiones y cambios sin fusionar los dos historiales de undo.

## D09 — Multilenguaje desde la base

**Aceptada.** ABL tiene prioridad de integración pero no exclusividad. Los proveedores de lenguaje permanecen fuera del núcleo de bytes.

## D10 — TUI y GUI comparten modelo de aplicación

**Aceptada.** Se crea `jc-app` como capa sin UI con `CommandRegistry`, `KeyMap`, `FocusManager`, menús, pestañas, paneles y diálogos.

Reglas vinculantes:

- ningún widget define su propia combinación de teclas para una acción de negocio;
- menú y shortcut consultan el mismo `CommandId`;
- los bindings contextuales pueden sombrear uno global únicamente por scope explícito;
- `Esc` cierra primero overlays y después devuelve el foco al editor;
- `F6`/Shift+F6 recorren solo áreas primarias visibles;
- `Tab` simple no cambia de área ni pestaña;
- cerrar una pestaña sucia exige una decisión explícita;
- comandos Progress permanecen deshabilitados fuera de contexto;
- TUI y GUI deben consumir este modelo común, no duplicarlo.

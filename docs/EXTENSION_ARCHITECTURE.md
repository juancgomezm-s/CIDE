# Arquitectura de capacidades y extensiones multilenguaje

**Estado:** contrato de diseño para M2-M5. El sistema de carga de plugins externos aún NO está implementado; se reserva para el anexo post-M5. No confundir una extensión interna de lenguaje con un plugin externo instalable.

## Regla central

JC-IDE es un editor general. El núcleo universal no debe saber cómo funciona la sintaxis, compilador, base de datos ni generadores de Progress ABL.

**Tres niveles distintos:**

| Nivel | Responsabilidad | Activación |
|---|---|---|
| Comandos universales | Archivo, edición, undo/redo, tabs, selección rectangular, búsqueda literal, navegación, diffs y tareas genéricas | Siempre |
| Capacidad de lenguaje | Nombre/extensión, delimitadores de comentarios, sangría, snippets y resaltado; servicios de sintaxis bajo demanda | Por documento y capacidad |
| Extensión especializada | AVM/compilador, XREF, esquema, firmas, jerarquías OO, code actions, herramientas de un ecosistema | Solo cuando se solicita y existe el proveedor |

El mismo `CommandRegistry`, `KeyMap`, `MenuModel` y modelo de foco gobiernan TUI y GUI. Una extensión contribuye comandos; **no** instala interceptores independientes de teclado, ni se apropia del buffer.

## Estado de la implementación

- `jc-core` mantiene los bytes canónicos y un historial, independiente de lenguajes.
- `jc-text` convierte coordenadas y codificaciones por documento.
- `jc-files` gobierna guardado y conflictos.
- `jc-app` mantiene estado de aplicación y comandos universales.
- `jc-languages` ya contiene perfiles de 18 lenguajes. **Primer paso implementado en fuente:** delimitadores de comentario de bloque y línea son metadatos de los perfiles, consultados por `jc-app` sin verificar `language.id == "abl"`.
- `jc-progress` ya aísla el protocolo de AVM/diagnósticos, aunque todavía no está envuelto por un contrato de extensión dinámica y sus auxiliares ABL requieren pruebas en instalaciones reales.

El modelo inicial `PaneManager` y el historial de navegación por grupo pertenecen a la capa universal `jc-app`. Los providers ABL/C#/Rust solo contribuyen destinos de símbolos; Git es otro proveedor transversal de snapshots/diffs, no una extensión de sintaxis. Ver [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).

Los comandos `ProgressCheck`, `ProgressCompile`, `ProgressRun` y `ProgressRefreshSchema` siguen presentes en el enum fijo de `jc-app`; son **deuda de modularización**, no una API de extensión implementada. Se migrarán al registro de contribuciones del paquete ABL cuando exista el contrato estable. No crear una segunda ruta de ejecución mientras tanto.

## Capacidades universales que nunca pertenecen a Progress

- Duplicate / Delete Line, Move Line, Tab / Shift+Tab, selección y modo columna.
- Find / Replace, búsqueda de archivos, glob, resultados tipados, navegación.
- Markers, árbol de proyecto, pestañas, diffs y sistema de comandos.
- Toggle Comment como **operación universal**: consulta el comentario elegido por el perfil del lenguaje; no codifica los delimitadores dentro del editor.
- Snippet engine genérico: la extensión aporta plantillas; el motor resuelve variables, vista previa, ubicación y undo.

En particular, **Search Plus** debe tener un motor textual transversal. Progress solo aportará un proveedor semántico especializado que consuma XREF/esquema, sin sustituir la búsqueda básica.

## Distribución propuesta para las extensiones internas

```text
jc-core / jc-text / jc-files
                 │
              jc-app
           /      |      \
       jc-tui   jc-gui   comandos
                 │          │
             jc-languages   │
                 └──────┬───┘
                        │ API de capacidades (pendiente)
             ┌──────────┴───────────┐
             │                      │
      progress.abl              csharp.dotnet
      (extensión)              (extensión)
       ├── snippets              ├── snippets
       ├── keywords              ├── format/LSP
       ├── parser/XREF            ├── tasks
       ├── schema                └── debug
       └── AVM
```

Las rutas de carpeta y nombres `progress.abl`/`csharp.dotnet` son **propuestos**, no módulos ya publicados. La implementación inicial puede enlazarlos estáticamente como componentes opcionales; una runtime universal de plugins no es un requisito para M3.

### Contrato mínimo que se diseñará antes de añadir acciones especializadas

- Identificador de extensión y comandos *namespaced* (ejemplo: `progress.abl.correctCase`).
- Capacidades declaradas: `comment`, `snippets`, `format`, `diagnostics`, `compile`, `schema`, `symbols`, `codeActions`.
- Restricciones por lenguaje, tipo de archivo y versión de toolchain; disponibilidad computada en un único lugar.
- Un proveedor de edición recibe **snapshot inmutable**, rangos de bytes, codificación y revisión esperada. Devuelve propuesta de ediciones tipada, no muta buffers directamente.
- El host valida revisión, no-superposición de rangos, codificación, permisos y cambios sobre los límites autorizados, y aplica el conjunto como **una transacción de undo**.
- Operaciones caras (compile, parser, symbol index) usan tareas cancelables, resultado tipado y presupuestos de recursos; jamás bloquean el teclado.
- Menús y atajos se construyen a partir del registro central y fallan con diagnóstico ante conflictos de shortcuts.
- Un proveedor ausente o incompatible no impide abrir/editar el archivo.

La API externa para plugins de terceros, su host aislado, permisos persistentes y compatibilidad Sublime **se diseñan e implementan solo después de M5**. No confundir este contrato interno con un motor Python.

## Paquete Progress ABL: catálogo funcional

| Capacidad | Funciones que aporta |
|---|---|
| Edición ABL ligera | snippets Procedure/Function/Trigger, Surround With, corrector de keywords |
| Análisis y formato | lexer ABL por versión, indentación sintáctica, imports USING, firmas |
| Estructura OO | constructors/methods/properties, herencia, override/implement |
| Adaptador de runtime | Check Syntax, Compile, Run, manejo de .pf/.ini, PROPATH, cpstream/cpinternal |
| Navegación y datos | XREF/XREF-XML, esquema de tablas/campos/índices, símbolos y diagnósticos |
| Herramientas riesgosas | AppBuilder cleanup, solo opcional con diff, backup y confirmación |

Se conserva el legado 10.0B como perfil distinto: no ofrecer sintaxis/OO que no soporte esa versión.

## Ruta de ejecución

1. Abrir `customer.p`: seleccionar perfil ABL y metadatos ligeros de comentarios. **No iniciar AVM**.
2. `Ctrl+/`: `edit.toggleComment` consulta el perfil y transforma el documento universalmente.
3. Solicitar Correct Case: resolver `progress.abl.correctCase`, activar tokenizador de ABL, proponer un diff sobre snapshot.
4. Solicitar Compile: cargar/verificar perfil y arrancar AVM bajo demanda mediante `jc-progress`; mostrar diagnostics/output como tareas tipadas.
5. Abrir `Program.cs`: la extensión ABL no participa; `csharp.dotnet` puede aportar sus servicios si está disponible.

## Milestones y límites

- **M2A:** consolidar edición/selección/búsqueda universal; perfiles con metadatos de comentarios; no instalar runtime de extensiones.
- **M2B:** la GUI consume los mismos comandos/capacidades.
- **M3:** contrato estable de proveedor interno, empaquetar utilidades ABL y levantar AVM solo bajo demanda.
- **M4:** capacidades ABL de análisis estructural y semántica, además de otros proveedores según necesidad.
- **M5:** estabilizar la interfaz de capacidades, mediciones de memoria/arranque, pruebas transversales y CI según D13.
- **Post-M5:** API de plugins de terceros y adaptadores Sublime, con aislamiento y permisos conforme al anexo existente.

**No se realizará ninguna ejecución de GitHub Actions durante M2–M4.** Si el entorno no tiene toolchain Rust, los cambios se señalan como no compilados, sin inventar validación.

# JC-IDE

Editor de código ligero, con núcleo Rust, soporte de varios lenguajes e integración
prioritaria con Progress OpenEdge/ABL. El diseño conserva los bytes y la codificación
original de los fuentes, incluidos los que no son UTF-8.

**Estado: M2A en desarrollo — núcleo de archivos/texto e interacción cerrados; TUI funcional en construcción. GUI pendiente.**
Este repositorio reúne la investigación, decisiones, plan y una implementación inicial
real. No es aún un reemplazo de PIEW, Sublime Text, Notepad++ o VS Code.

## Qué contiene

| Componente | Implementado en esta base |
|---|---|
| `jc-core` | Piece table de bytes, edición, revisiones, undo/redo acotado y compactación explícita |
| `jc-text` | UTF-8/Windows-1252/ISO-8859-1/IBM850, decodificación no destructiva, line endings y byte↔línea/columna |
| `jc-files` | Sesión de archivo, detección de cambios externos, guardado atómico y creación no-clobber |
| Selecciones | Port parcial de `Region` de Lime Text, con procedencia y licencia conservadas |
| `jc-languages` | 18 perfiles y detección por extensión/nombre de archivo |
| `jc-progress` | Construcción de argumentos sin shell, solicitudes ASCII y parser acotado de diagnósticos/esquema |
| `jc-app` | Registro central de comandos, keymap contextual, foco, menús, pestañas, paneles y diálogos compartidos por TUI/GUI |
| Auxiliares ABL | Código experimental para comprobar/compilar y exportar esquema; requiere pruebas con Progress real |
| `jc-cli` | Inspección de archivos, copia exacta, edición de bytes a copia nueva, demo de undo/redo, preparación de solicitudes y validación de respuestas Progress |
| Documentación | Resumen, investigación, arquitectura, decisiones, hitos, lenguajes y evidencia de validación |

El núcleo y los otros crates usan únicamente la biblioteca estándar de Rust y dependencias
internas del workspace. La piece table usa un vector de fragmentos en esta primera versión;
las operaciones recorren O(P) fragmentos. El historial tiene límite configurable (1024 por defecto) y el almacenamiento añadido puede compactarse explícitamente.

## Empezar

Instala [Rust](https://rustup.rs/), clona el repositorio y abre su carpeta. Se fija el
toolchain 1.85.1; en Windows, usa el entorno de compilación MSVC requerido por Rust.

```console
cargo test --workspace --locked --offline
cargo run -p jc-cli --offline -- demo
cargo run -p jc-cli --offline -- languages
cargo run -p jc-cli --offline -- inspect ruta/al/programa.p
```

Copiar un documento sin decodificar ni alterar finales de línea:

```console
cargo run -p jc-cli --offline -- roundtrip entrada.p copia-nueva.p
```

Insertar un byte `F1` en la posición 0 de una **copia nueva**, conservando el original:

```console
cargo run -p jc-cli --offline -- edit entrada.p 0 0 F1 copia-editada.p
```

Los offsets del núcleo son bytes, no caracteres ni columnas visuales. `jc-text` realiza las conversiones de coordenadas de forma explícita y `jc-files` implementa el ciclo de guardado seguro/atómico para el futuro frontend. La CLI de demostración conserva sus salidas `create_new` deliberadamente.

## Política de CI

El CI remoto se reserva para **M5 (hardening)**. Durante M2–M4 GitHub Actions no se dispara automáticamente por push/PR y no debe utilizarse como bucle de depuración de código todavía inestable. El workflow permanece manual y dormido hasta M5; la validación intermedia se hace local cuando el entorno lo permite.

## Varios lenguajes

Perfiles iniciales: **ABL, C#, Rust, JavaScript/JSX, TypeScript/TSX, Python, HTML, CSS,
SQL, XML, JSON, JSON con comentarios, Markdown, TOML, YAML, PowerShell, Shell y texto**.

Actualmente se reconocen los perfiles y se pueden editar sus archivos como bytes.
**Resaltado, parsers, completado semántico, LSP y depuración aún no están implementados.**
El núcleo es independiente del lenguaje y la codificación se elegirá por documento;
véase [docs/LENGUAJES.md](docs/LENGUAJES.md).

## Progress: integración pequeña, entorno real

El modelo toma como referencia la integración de PIEW: utilizar el compilador y la AVM
de Progress, consultar el esquema mediante ABL y mantener un catálogo local para navegación.
Cada proyecto seleccionará su ejecutable, `.pf`, `.ini`, `PROPATH` y páginas de códigos.

En M0, `progress-plan` **prepara una solicitud y muestra ejecutable/argumentos; no inicia
Progress, no conecta a una BD y no ejecuta programas de negocio**. Los auxiliares ABL y
la prueba manual están documentados en [progress/README.md](progress/README.md) y
[docs/PROGRESS_BRIDGE.md](docs/PROGRESS_BRIDGE.md). La ejecución de programas que requieren
el contexto de un ERP se implementará con perfiles de arranque explícitos.

La compatibilidad con OpenEdge 10.0B y 11.7 es un objetivo pendiente de validación sobre
esas instalaciones. El transporte inicial admite rutas/PROPATH ASCII; esa restricción
no cambia los bytes ni la codificación del contenido de los fuentes. No se asume que los
fuentes del usuario sean Windows-1252, ISO-8859-1 o IBM850.

## Leer primero

1. [Resumen del proyecto](docs/RESUMEN.md)
2. [Plan por hitos y criterios de aceptación](docs/PLAN.md)
3. [Arquitectura](docs/ARQUITECTURA.md)
4. [Decisiones y pendientes](docs/DECISIONES.md)
5. [Investigación de Lime Text, PIEW y Progress](docs/INVESTIGACION.md)
6. [Contrato de interacción TUI](docs/TUI_INTERACTION.md)
7. [M1: archivos, encoding y posiciones](docs/M1_FILES_TEXT.md)
8. [Validación observada y límites](docs/VALIDACION.md)
9. [Post-M5: compatibilidad Sublime Text](docs/POST_M5_SUBLIME_COMPAT.md)

## Procedencia y licencia

La parte portada de Lime se limita a las operaciones de `Region` indicadas en el código
y sus casos de compatibilidad. El buffer, historial, CLI y adaptador son trabajo nuevo.
No se incorpora la aplicación PIEW ni su motor gráfico.

La licencia de distribución del código nuevo está pendiente de decisión del propietario.
Las condiciones BSD de la parte derivada de Lime se conservan: [LICENSING.md](LICENSING.md).

Consulta [CONTRIBUTING.md](CONTRIBUTING.md) para construir, probar y ampliar el proyecto.

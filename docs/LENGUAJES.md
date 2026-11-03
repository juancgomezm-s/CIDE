# Soporte de lenguajes

## Qué existe en M0

El buffer acepta bytes de cualquier documento sin imponer el lenguaje ni la codificación.
`jc-languages` ofrece 18 perfiles y detección por nombre/extensión. El resultado es una
**sugerencia de perfil**, no un análisis del contenido ni una certificación de sintaxis.
Un perfil explícito de documento o proyecto debe tener prioridad cuando exista la UI.

| Perfil | Ejemplos |
|---|---|
| Progress ABL | `.p`, `.w`, `.i`, `.cls` |
| C# | `.cs`, `.csx` |
| Rust | `.rs` |
| JavaScript / JSX | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript / TSX | `.ts`, `.tsx`, `.mts`, `.cts` |
| Python | `.py`, `.pyw`, `.pyi` |
| HTML | `.html`, `.htm` |
| CSS | `.css` |
| SQL | `.sql` |
| XML | `.xml`, `.xsd`, `.xsl`, `.xslt`, `.csproj`, `.props`, `.targets` |
| JSON | `.json` |
| JSON con comentarios | `.jsonc`, `tsconfig.json`, `jsconfig.json` |
| Markdown | `.md`, `.markdown` |
| TOML | `.toml` |
| YAML | `.yaml`, `.yml` |
| PowerShell | `.ps1`, `.psm1`, `.psd1` |
| Shell | `.sh`, `.bash`, `.zsh`, `.bashrc`, `.zshrc` |
| Texto | `.txt`, `.log` y nombres desconocidos |

Los nombres de archivo específicos se comprueban antes que la extensión. Se reconocen
separadores de ruta Windows y Unix, y extensiones sin distinguir mayúsculas. Extensiones
ambiguas como `.cls` se interpretan según la prioridad ABL del proyecto; será posible
sobrescribir esa sugerencia. No se supone que ABL antiguo admita comentarios `//`.

```console
cargo run -p jc-cli --offline -- languages
cargo run -p jc-cli --offline -- inspect programa.p
```

## Lo que falta

El registro incluye resaltado léxico compartido para ABL, Rust, JSON y JSONC en TUI y GUI.
Los otros perfiles usan Plain. Son subconjuntos léxicos acotados, no gramáticas completas;
[contrato, límites y evidencia](M2_SYNTAX_HIGHLIGHTING.md). No incluye parseo incremental,
completado semántico, diagnósticos de C#/Rust/etc., procesos LSP, compiladores genéricos ni depuración.
El autocompletado de BD también está pendiente: hay un contrato y exportador experimental,
pero no una interfaz que lo consuma en tiempo real.

## Contrato de arquitectura

Cada lenguaje podrá ofrecer por separado resaltado, símbolos, diagnósticos, completado,
formateo y ejecución. Un servicio solo se inicia cuando una función habilitada lo necesita.
El editor debe continuar funcionando si el compilador o LSP no está instalado.

El proveedor ABL reutilizará Progress y su esquema; otros proveedores podrán usar LSP o
comandos de compilación. Un registro no debe ejecutar programas por abrir una extensión.
La codificación permanece en el documento: puede haber un `.p` legacy y un JSON UTF-8
abiertos al mismo tiempo, con conversiones explícitas en los límites de cada servicio.

Capacidades TUI posteriores: completado textual core.words para cualquier perfil
y símbolos/referencias inferidos ABL mediante progress.abl opcional. No completado
semántico ni parser certificado; [uso y límites](M2_SOURCE_SERVICES.md).

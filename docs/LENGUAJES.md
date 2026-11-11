# Soporte de lenguajes

## Perfiles de archivo

El buffer acepta bytes de cualquier documento sin imponer el lenguaje ni la codificación.
`jc-languages` ofrece 20 perfiles y detección por nombre/extensión. El resultado es una
**sugerencia de perfil** que activa las capacidades del editor.
Un perfil explícito de documento o proyecto debe tener prioridad cuando exista la UI.

| Perfil | Ejemplos |
|---|---|
| Progress ABL | `.p`, `.w`, `.i`, `.cls` |
| C# | `.cs`, `.csx` |
| Rust | `.rs` |
| JavaScript / JSX | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript / TSX | `.ts`, `.tsx`, `.mts`, `.cts` |
| PHP | `.php`, `.phtml`, `.inc` |
| Java | `.java` |
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

## Capacidades actuales

GUI y TUI resaltan ABL, Rust, JSON, JSONC, Markdown, C#, JavaScript/JSX,
TypeScript/TSX, Java y PHP. C#, JavaScript, TypeScript, Java y PHP aportan
símbolos y dependencias estáticas al panel Esquema. Node.js comparte el
perfil JavaScript y aporta referencias literales CommonJS. [Configuración y alcance](EXTENSION_ARCHITECTURE.md#resaltado-y-navegación-multilenguaje).

GUI y TUI ofrecen **Ir a definición** y **Buscar referencias** con un servidor
LSP configurado para C#, JavaScript/TypeScript, PHP o Java.
[Configuración común y atajos](M3_LSP_NAVEGACION.md).

ABL tiene además comprobación, compilación, ejecución por lotes y consulta
del catálogo de bases. El completado semántico, los compiladores genéricos y
la depuración siguen pendientes. [Perfil Progress](PROGRESS_BRIDGE.md).

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

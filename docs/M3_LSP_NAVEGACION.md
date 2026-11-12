# Servicios semánticos con LSP

En la GUI y la TUI, abre la carpeta del proyecto y un archivo guardado de C#, JavaScript, TypeScript, PHP o Java. Coloca el cursor sobre un símbolo y usa **Ir a definición (LSP)** (`F12`) o **Buscar referencias (LSP)** (`Mayús+F12`). Los comandos también están en la paleta y en **Navegación** de la GUI o **Proyecto** de la TUI. Selecciona una ubicación y pulsa `Intro` para abrirla o `Ctrl+Intro` para abrirla al lado. La consulta de referencias incluye la declaración cuando el servidor la entrega.

Usa **Editar → Autocompletado semántico (LSP)** (`Ctrl+Mayús+Espacio`) para pedir sugerencias en el cursor. `Intro` aplica la sugerencia y las ediciones adicionales recibidas con ella en un solo paso de deshacer. **Herramientas → Comprobar con LSP** (`Ctrl+Mayús+M`) revisa el documento activo y abre **Problemas**. `Intro` sobre una fila lleva al rango del diagnóstico. Al editar se retiran las filas LSP que describían la revisión anterior. Ejecuta la comprobación otra vez para actualizar el panel. Las filas de compilación ABL tienen su propia procedencia.

## Configuración compartida

Guarda un archivo JSON UTF-8 en `%LOCALAPPDATA%\CIDE\lsp.json`. La variable `CIDE_LSP_CONFIG` permite indicar otra ruta absoluta. El archivo es local al usuario y debe apuntar a servidores de confianza. CIDE inicia el proceso al solicitar un comando LSP. El servidor se ejecuta en la carpeta del proyecto y puede leerla e indexarla según su propia configuración.

```json
{
  "servers": {
    "csharp": {
      "command": "C:/herramientas/csharp-ls.exe"
    },
    "ecmascript": {
      "command": "C:/Program Files/nodejs/node.exe",
      "args": ["C:/herramientas/node_modules/typescript-language-server/lib/cli.mjs", "--stdio"],
      "initializationOptions": {
        "disableAutomaticTypingAcquisition": true,
        "tsserver": { "useSyntaxServer": "never" }
      }
    },
    "php": {
      "command": "C:/Program Files/nodejs/node.exe",
      "args": ["C:/herramientas/node_modules/intelephense/lib/intelephense.js", "--stdio"]
    },
    "java": {
      "command": "C:/herramientas/jdk-21/bin/java.exe",
      "args": [
        "-Declipse.application=org.eclipse.jdt.ls.core.id1",
        "-Dosgi.bundles.defaultStartLevel=4",
        "-Declipse.product=org.eclipse.jdt.ls.core.product",
        "--add-modules=ALL-SYSTEM",
        "--add-opens", "java.base/java.util=ALL-UNNAMED",
        "--add-opens", "java.base/java.lang=ALL-UNNAMED",
        "-jar", "C:/herramientas/jdtls/plugins/ARCHIVO_LAUNCHER.jar",
        "-configuration", "C:/herramientas/jdtls/config_win",
        "-data", "C:/herramientas/espacio-jdtls"
      ]
    }
  }
}
```

Configura solo los lenguajes que uses y sustituye las rutas de ejemplo. JavaScript y TypeScript pueden compartir `ecmascript` o definir entradas `javascript` y `typescript` propias. Cada entrada admite `settings` para responder a consultas de configuración del servidor. En Java, elige el nombre real del JAR y una carpeta `-data` propia por servidor concurrente. Las instrucciones de cada servidor detallan sus requisitos. [C#](https://github.com/razzmatazz/csharp-language-server) · [JavaScript y TypeScript](https://github.com/typescript-language-server/typescript-language-server) · [PHP](https://github.com/bmewburn/intelephense-docs) · [Java](https://github.com/eclipse-jdtls/eclipse.jdt.ls)

Para C#, abre la carpeta que contiene la solución `.sln` o `.slnx`. En TypeScript, `useSyntaxServer: "never"` usa el servidor semántico desde la primera consulta y evita que el servidor sintáctico devuelva solo la importación local durante el arranque.

## Alcance

CIDE usa el mismo cliente Rust en ambas interfaces. Mantiene un servidor activo por instancia y reutiliza la conexión para la misma carpeta y configuración. Envía el contenido actual del archivo bajo el cursor, incluidos los cambios sin guardar. Los resultados de otros archivos se verifican contra el contenido en disco y se vuelven a comprobar al abrirlos. La consulta caduca si cambia el documento, la revisión, la vista, el cursor o la raíz.

Este bloque presenta hasta 48 ubicaciones locales dentro del proyecto. Los destinos externos, virtuales y con codificación indeterminada se omiten con un aviso. Una consulta admite un documento de hasta 2 MiB y un plazo de 45 segundos. El cliente usa posiciones UTF-16 y mensajes JSON-RPC sobre entrada y salida estándar. [Especificación LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)

`serde_json` 1.0.145 y su dependencia `ryu` se fijan en `Cargo.lock` para codificar y validar JSON-RPC, con límites de tamaño aplicados antes de analizar las respuestas. Los servidores de lenguaje son programas externos elegidos por el usuario.

El completado admite inserciones de texto y cambios adicionales en el mismo documento. Las plantillas del servidor, los comandos adjuntos y la resolución diferida de sugerencias quedan fuera del bloque. Para diagnósticos se consulta al servidor cuando admite `textDocument/diagnostic` y se recibe su publicación para el archivo activo en los demás casos. Los informes sin versión se admiten en una sesión recién sincronizada para ese documento. Los mensajes de cada diagnóstico conservan el idioma que entrega el servidor.

## Validación local

Windows x64 y Rust 1.88. Los dos comandos devolvieron ubicaciones de definición y referencias con cinco proyectos de prueba: C# con `csharp-ls` 0.28.0 y una solución `.slnx` de .NET 10, JavaScript `.mjs` y TypeScript con `typescript-language-server` 5.1.3 y TypeScript 5.9.3, PHP con Intelephense 1.15.2 y Java con Eclipse JDT LS 1.52.0 y JDK 21. Las dos interfaces probaron la apertura lateral con TypeScript. La GUI comprobó además la sincronización de una edición Unicode sin guardar y la conservación del archivo en disco.

Pasaron 49 casos dirigidos y los siete recorridos con servidores reales. También pasaron `python -B scripts/check-local.py --steps fmt clippy --jobs 2` y la compilación optimizada del workspace con dependencias bloqueadas y sin red. Los ejecutables están en `target/lsp-navigation-next/release`.

El bloque de autocompletado y diagnósticos pasó siete recorridos adicionales con los mismos servidores. Los cinco proyectos verificaron sugerencia, un solo deshacer, diagnóstico navegable e invalidación al editar. La GUI y la TUI repitieron el flujo con TypeScript y abrieron **Problemas**. Pasaron formato y Clippy del workspace. Se comprobó la convivencia de filas ABL y LSP y el descarte de publicaciones de otra versión o documento.

Las pruebas con servidor externo se ejecutan de forma explícita. Usa `CIDE_LSP_CONFIG` para la configuración local, `CIDE_LSP_TEST_PROJECT` para la carpeta absoluta de prueba y `CIDE_LSP_TEST_FILE` y `CIDE_LSP_TEST_DEST` para las rutas relativas del archivo con la llamada y de la declaración. El símbolo de prueba se llama `calcular`.

```powershell
cargo test --locked --offline -p jc-app --lib lsp_server_roundtrip -- --ignored --nocapture
cargo test --locked --offline -p jc-gui --lib source_gui_lsp_navigation -- --ignored
cargo test --locked --offline -p jc-tui --lib source_tui_lsp_navigation -- --ignored
```

Para el bloque de completado y diagnósticos, usa los filtros `lsp_language_services_roundtrip`, `source_gui_lsp_language_services` y `source_tui_lsp_language_services`. El archivo de prueba debe tener un prefijo `calcu` que se complete como `calcular` y una referencia sin declarar llamada `missingCideSymbol`.

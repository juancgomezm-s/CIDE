# Anexo post-M5 — Compatibilidad con plugins y extensiones de Sublime Text

**Inicio:** únicamente después de cerrar M5.

Este anexo define una línea de evolución posterior al roadmap principal. Su objetivo es permitir que JC-IDE aproveche una parte significativa del ecosistema de Sublime Text sin convertir al editor en una reimplementación de Sublime ni introducir su modelo de plugins dentro del núcleo.

## Relación con extensiones internas

El contrato de capacidades de lenguaje se prepara en M2–M5 para que `progress.abl` y otros proveedores usen comandos centrales. Este anexo es distinto: **solo después de M5** se permitirá cargar extensiones de terceros, paquetes Sublime y plugins Python mediante host aislado. Ver [EXTENSION_ARCHITECTURE.md](EXTENSION_ARCHITECTURE.md).

## Principios

1. **El core no conoce Sublime.** `jc-core`, `jc-text` y `jc-files` permanecen independientes.
2. **Todo se adapta a los contratos de JC-IDE.** Comandos, menús y atajos terminan en `CommandId`; lenguajes y sintaxis pasan por `jc-languages`.
3. **Compatibilidad por niveles, no promesa de 100 %.** Cada formato/API declara qué subconjunto está soportado.
4. **Datos antes que código.** Se implementan primero recursos declarativos; ejecutar plugins Python es la última etapa.
5. **Seguridad explícita.** Un paquete no obtiene filesystem, procesos, red o shell implícitamente.
6. **TUI y GUI comparten backend.** Las extensiones no deben implementar dos lógicas distintas para cada frontend.
7. **Fallos aislados.** Un plugin incompatible no puede derribar el editor ni corromper el documento.

## Fase A — Recursos declarativos

Primera prioridad por coste/riesgo bajo y valor inmediato.

Formatos candidatos:

- `.sublime-syntax`;
- `.tmLanguage` cuando sea útil como formato legado;
- `.sublime-color-scheme` y, opcionalmente, `.tmTheme`;
- `.sublime-settings`;
- `.sublime-keymap`;
- `.sublime-menu`;
- `.sublime-snippet`;
- `.sublime-completions`;
- `.sublime-build`.

### Reglas

- Los keymaps se traducen al `KeyMap` central; nunca crean un segundo sistema de shortcuts.
- Los menús se traducen al `MenuModel`.
- Los builds se convierten en tareas/commands con argumentos explícitos; no se ejecutan por abrir un proyecto.
- Las sintaxis se usan solo para highlighting/parsing donde su semántica sea compatible.
- La precedencia entre configuración de JC-IDE y Sublime debe quedar documentada y determinista.

## Fase B — Paquetes Sublime

Agregar soporte para cargar paquetes desde:

- directorios desempaquetados;
- archivos `.sublime-package` compatibles;
- un catálogo de paquetes instalado por el usuario.

Cada paquete tendrá manifiesto interno de JC-IDE con:

- nombre y versión;
- formatos encontrados;
- comandos registrados;
- permisos solicitados;
- frontend requerido: ninguno / TUI / GUI / ambos;
- estado de compatibilidad;
- errores de carga.

No se cargará código ejecutable automáticamente por encontrar un paquete.

## Fase C — API de compatibilidad para plugins Python

Esta es la parte de mayor coste y se implementará después de demostrar valor con A/B.

La meta será un **subconjunto documentado** de las APIs `sublime` y `sublime_plugin`, no una emulación ciega de todas las versiones históricas.

Áreas candidatas:

- `TextCommand`;
- `WindowCommand`;
- `ApplicationCommand`;
- listeners de eventos seleccionados;
- View/Window básicos;
- Settings;
- Region/Selection;
- comandos y argumentos;
- status messages;
- quick panels/input panels cuando puedan mapearse al modelo común.

### No asumir compatibilidad automática

APIs fuertemente ligadas al renderer de Sublime, internals privados, comportamiento no documentado o plugins que modifiquen directamente widgets pueden quedar:

- no soportados;
- parcialmente soportados;
- disponibles solo en GUI;
- reemplazados por una API nativa de JC-IDE.

## Runtime y seguridad

Los plugins Python deben ejecutarse fuera del núcleo del editor.

Diseño objetivo:

```text
Sublime package
      │
      ▼
jc-sublime-host
  Python runtime
      │
      │ RPC tipado
      ▼
jc-plugin-api
      │
      ├── CommandRegistry
      ├── Documents
      ├── Languages
      ├── Panels
      └── Tasks
```

El host debe poder reiniciarse o terminarse sin cerrar JC-IDE.

Capacidades sensibles requerirán permisos explícitos, por ejemplo:

- leer fuera del workspace;
- escribir archivos;
- ejecutar procesos;
- shell;
- red;
- acceso a secretos/configuración;
- integración con bases de datos.

## Compatibilidad de comandos y atajos

Un plugin Sublime no puede registrar directamente un handler de teclado.

Flujo obligatorio:

```text
.sublime-keymap / plugin
        ↓
compatibility adapter
        ↓
CommandId / plugin command
        ↓
KeyMap + CommandRegistry
        ↓
TUI / GUI
```

Así se conservan las reglas ya definidas para conflictos, scopes, Command Palette y menús.

## Package Control

La compatibilidad con Package Control se evaluará como trabajo separado.

Opciones:

1. leer repositorios/metadatos compatibles y descargar paquetes;
2. importar paquetes ya instalados desde una instalación local de Sublime;
3. mantener un catálogo propio de paquetes verificados para JC-IDE.

No se ejecutará código remoto durante instalación sin inspección y permisos.

## Matriz de compatibilidad

Cada paquete deberá poder mostrar algo equivalente a:

| Capability | Estado |
|---|---|
| Syntax | Compatible |
| Snippets | Compatible |
| Keymap | Compatible con remapeo |
| Python API | Parcial |
| Filesystem | Requiere permiso |
| Processes | No concedido |
| TUI | Compatible |
| GUI | Compatible |

Esto evita el estado ambiguo de “plugin instalado” cuando solo funciona una parte.

## Criterios de aceptación del anexo

- instalar/desinstalar un paquete sin reiniciar el core;
- cargar recursos declarativos sin runtime Python cuando no sea necesaria;
- traducir keymaps/menus al sistema central sin shortcuts duplicados ocultos;
- aislar plugins Python en un proceso/host separado;
- permisos visibles y revocables;
- un crash del host no pierde cambios del documento;
- mostrar compatibilidad y errores por paquete;
- probar un conjunto representativo de plugins reales de distintas categorías;
- medir RAM y tiempo de arranque con cero plugins y con plugins activos;
- mantener JC-IDE plenamente utilizable con todo el subsistema Sublime deshabilitado.

## Fuera de alcance inicial

- compatibilidad perfecta con cada versión histórica de Sublime Text;
- ejecución de módulos binarios arbitrarios;
- replicar internals privados;
- garantizar que un plugin orientado exclusivamente a UI gráfica funcione en TUI;
- incorporar Python al proceso principal de JC-IDE.

## Resultado esperado

JC-IDE debe poder adoptar sintaxis, snippets, themes, keymaps, build systems y una selección creciente de plugins Sublime sin sacrificar sus objetivos originales de ligereza, aislamiento, teclado coherente y núcleo independiente.

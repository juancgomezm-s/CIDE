# CIDE: nombre visible y compatibilidad

La decisión del propietario cambia la marca visible de **JC-IDE** a **CIDE**.
No define una expansión nueva del nombre ni cambia el alcance funcional de los
frontends. La CLI sigue siendo headless; la GUI nativa es un ejecutable separado.

| Interfaz | Comando canónico | Alias histórico compatible |
| --- | --- | --- |
| CLI | `cide` | `jc-ide` |
| TUI | `cide-tui` | `jc-ide-tui` |
| GUI | `cide-gui` | `jc-ide-gui` |

Los aliases deben usar la misma implementación y los mismos argumentos. La
invocación histórica no conserva una marca distinta: ayuda, títulos y prefijos de
error presentan CIDE y el nombre canónico correspondiente. Los manifiestos fijan
default-run canónico; ambos puntos de entrada llaman al mismo módulo entry.rs.
El alias no duplica las pruebas unitarias del ejecutable.

## Cambios de este frente

- [CLI](../crates/jc-cli/src/entry.rs): ayuda con `cide`, alias histórico explícito,
  prefijo de error `cide:` y comentario de la demostración `/* CIDE */`. La ayuda
  deja de afirmar que no existe GUI y señala `cide-gui` como ejecutable separado.
- [TUI](../crates/jc-tui/src/entry.rs): título y uso `CIDE TUI` / `cide-tui`, alias
  histórico explícito y prefijos de error canónicos.
- [Composición TUI](../crates/jc-tui/src/render/mod.rs): bienvenida, estado sin
  documento y aviso de terminal pequeña muestran CIDE; la comprobación existente
  de bienvenida se alinea con la marca actual.
- [Overlays](../crates/jc-tui/src/render/overlays.rs): el selector de encoding usa
  CIDE. [Comandos](../crates/jc-tui/src/app/commands.rs): el aviso de shell se
  refiere específicamente a CIDE TUI, sin atribuir su interfaz al producto entero.

- GUI: ayuda, ventana, cabecera y diálogos muestran CIDE. El HWND conserva el título
  suministrado al delegar WM_NCCREATE a DefWindowProc después de enlazar su owner.
- README, documentación de uso vigente, scripts y descripciones de paquetes usan
  CIDE. Cargo.lock, nombres de crates y APIs se conservan.
- [Iconos](../assets/branding/README.md): original JG+CIDE del paquete para TUI/CLI;
  variante naranja para GUI, incluyendo sus aliases. Siete tamaños por PE; recurso
  grupo101 cargado por la ventana. Sin dependencia Rust, instalación ni rc externo.
- `scripts/package-cide.py` crea un ZIP local de desarrollo explícito con seis
  ejecutables, recursos, licencias disponibles y manifiesto SHA256. No instala ni
  publica. Inventario offline de notices con ausencias explícitas, no revisión legal.

## Identidad interna y evidencia

Los paquetes y crates (`jc-app`, `jc-tui`, `jc-gui`, etc.), imports Rust,
identificadores de documento/vista, protocolos, claves de configuración y nombres
internos conservan su identidad. No hay migración de configuración o datos por
esta decisión de marca.

No se renombra la carpeta de trabajo `JC-IDE`, el repositorio, su URL/remoto ni
rutas ajenas. Tampoco se cambian nombres de fixtures temporales, formatos de
protocolo como `JCIDE`, licencias, atribuciones o identificadores de proveedores.
Las rutas históricas son evidencia y no instrucciones para crear una nueva raíz.

Los informes, mediciones, capturas y commits anteriores pueden seguir diciendo
JC-IDE: describen la versión realmente observada. No se reescribe esa evidencia
para simular que se obtuvo con los ejecutables CIDE. La documentación de uso
actual adopta los comandos canónicos y conserva la compatibilidad explícita.

## Validación

El cierre integrado aprobó391 pruebas distintas por suites, fmt, Clippy y release.
`scripts/check-branding.py` ejecutó las seis ayudas: exit0 y ayudas idénticas por
pareja canónico/alias. `scripts/check-cide-icons.py` verificó en los seis PE los
siete frames16..256 contra los ICO fuente, cargados como datos sin ejecutar código.
El recorrido nativo41 verifica título y carga del icono en el HWND propio.
[Ayudas](evidence/cide-project-native-2026-11-02-branding.json),
[recursos](evidence/cide-project-native-2026-11-02-cide-icons.json) y
[reporte nativo](evidence/cide-project-native-2026-11-02-native-report.json).
La prueba no acredita pantalla física, interoperabilidad de clipboard, IME,
accesibilidad, DPI ni metas globales de RAM/latencia. Sin CI/push/dependencias nuevas.

Paquete observado: `work/packages/CIDE-windows-dev.zip`,69 entradas,3480193 bytes;
CRC y todos los hashes del manifiesto verificados. El inventario conservador
offline marca dos notices ausentes de winapi GNU (i686/x86_64), sin ocultarlos ni
afirmar que sean dependencias enlazadas por este build MSVC.
[Hash y resultado de empaquetado](evidence/cide-project-native-2026-11-02-package.json).

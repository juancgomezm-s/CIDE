# Motor Git embebido en Rust — revisión de arquitectura

Revisión del 5 de noviembre de 2026. Juan precisa que el motor también debe
viajar dentro de CIDE y funcionar sin una instalación de `git.exe`.
Esta revisión precede a la autorización de implementación. La lectura M2 ya
se sustituyó por el [motor embebido](GIT_EMBEDDED_ENGINE.md); ese documento
registra el contrato vigente, las cuotas y las limitaciones. El resto de esta
revisión conserva la justificación y el plan original, sin afirmar capacidades M5.

## Recomendación

Usar `gix`, biblioteca de gitoxide, enlazada dentro de CIDE. Es una implementación
Git en Rust y su API permite acceder directamente a repositorios, referencias,
objetos e índices. La aplicación utilizaría la biblioteca, no la herramienta
de consola `gix`. [Proyecto oficial](https://github.com/GitoxideLabs/gitoxide),
[API de biblioteca](https://docs.rs/gix/0.90.0/gix/).

| Opción | Sin Git instalado | Motor Rust | Encaje |
|---|---|---|---|
| gitoxide / `gix` enlazado | Sí, para las capacidades implementadas sin auxiliares | Sí | Candidato recomendado |
| `git2` + libgit2 enlazada | Sí | No; envoltura Rust de biblioteca C | No cumple el requisito de lenguaje |
| Empaquetar Git portable | No requiere instalación separada | No | Sigue ejecutando Git externo |

La distinción de `git2` está confirmada por su
[documentación oficial](https://github.com/rust-lang/git2-rs).

## Versión, dependencias y compilación

La versión publicada/documentada consultada es `gix 0.90.0`; su manifiesto declara
Rust mínimo 1.88, edición 2024 y licencia MIT o Apache-2.0. CIDE fija 1.85.1:
no es una integración compilable con la configuración actual. Se recomienda
evaluar una actualización controlada del toolchain y fijar el grafo completo.
No elegir una versión antigua solo para esquivar esa evaluación.
[Manifiesto de gix](https://github.com/GitoxideLabs/gitoxide/blob/main/gix/Cargo.toml).

Comenzar con `default-features = false` y habilitar únicamente lectura de objetos,
índice, revisión y estado. Revisar los hashes requeridos y la descompresión Rust;
no activar por comodidad redes, credenciales, mutaciones o paralelismo global.
Los nombres y transitivas exactos se fijarán tras resolver y auditar Cargo.lock.
No se ha medido todavía el coste en compilación, tamaño de binario o memoria.

## Qué puede sustituir en M2

El catálogo oficial identifica soporte de estado, comparación árbol/índice,
objetos y diffs; encaja con el alcance actual de lectura de CIDE. También
enumera trabajo pendiente de orquestación en checkout/restauración, merge,
rebase, push y transportes autosuficientes SSH/file. No implica paridad total
con Git ni autoriza anunciar esos flujos como terminados.
[Estado de capacidades](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md).

El primer bloque embebido debe cubrir:

1. Abrir la raíz, resolver HEAD y rama, leer objetos sueltos y empaquetados.
2. Estado de HEAD frente al índice y del índice frente al disco, más no rastreados
   e ignorados con la semántica del proveedor actual.
3. Leer blobs exactos de HEAD/índice y entregar los mismos modelos de comparación.
4. Mantener interfaz GUI/TUI y refresco por eventos, sin rutas de ejecutable.

No se reescribe el modelo de edición ni el diff de presentación. Se conservan
los bytes originales, las codificaciones elegidas, BOM/EOL y los buffers sucios.
Estado Git sigue reflejando disco/índice; la comparación del documento activo
sigue usando su snapshot versionado.

## Sustitución arquitectónica

Crear un proveedor `jc-git` independiente del núcleo de bytes. Trasladar allí
los contratos de repositorio, estado y blob; `jc-app` puede reexportar los tipos
durante la migración, sin mantener copias de los datos. Sus controladores y F3
seguirán admitiendo, cancelando y aceptando resultados por generación/revisión.

Sustituir `GitReader` y la captura porcelain por llamadas tipadas a gix. Estado
debe mapearse directamente a los modelos, no convertirlo a texto porcelain y
volver a parsearlo. Conservar un trabajo activo y el último pendiente, cuotas
y presentación de solo lectura. El repositorio y sus cachés pertenecen al worker.

La entrega final elimina de producción el runner de `git.exe`, su búsqueda,
el selector y opciones de ejecutable en ambos frontends. Git de consola puede
quedar como oráculo de pruebas de desarrollo, separado del producto distribuido.
Los metadatos de sesión antiguos deben leerse consumiendo/ignorando la ruta
histórica; un cambio de motor no debe impedir recuperar documentos sin guardar.

## Controles que requieren prueba antes de adoptar

La apertura aislada de gix restringe entorno y configuración externa, pero
permite configuración local del repositorio. No basta por sí sola para asegurar
que no se ejecutan auxiliares. Auditar filtros y otras opciones locales; en M2
rechazar los filtros externos como hoy, sin ejecutar hooks, helpers, fsmonitor,
red o mantenimiento. Mantener compatibilidad de worktrees/commondir admitida.
[Opciones de apertura](https://docs.rs/gix/0.90.0/gix/open/struct.Options.html).

La API de estado admite iteración e interrupción; el paralelismo puede introducir
operaciones simultáneas y orden no definido. Encajar su uso en el presupuesto
de workers de CIDE, ordenar el resultado de forma determinista y no persistir
actualizaciones del índice durante una consulta.
[Estado e interrupción](https://docs.rs/gix/0.90.0/gix/status/struct.Platform.html).

Un límite de salida no limita lo que gix pueda asignar al cargar un índice,
resolver deltas o descomprimir objetos. Aplicar preflight y medir esos picos;
conservar el límite de blobs actual de 2 MiB por defecto y errores visibles.
No afirmar RAM o plazos duros a partir del nombre de la biblioteca. La cancelación
en el proceso sería cooperativa y no permite matar un hilo bloqueado de forma segura.
Si no cumple las cotas, evaluar un worker Rust aislado iniciado desde el mismo
ejecutable CIDE: el motor seguiría distribuido dentro del producto y no requeriría
Git instalado. Esa alternativa debe medirse, no activarse por anticipado.

## Bloques de ejecución propuestos

**G0 — Prototipo de admisión.** Fijar toolchain/dependencias y licencias; construir
una prueba separada de lectura y estado. Medir tamaño, arranque, memoria, índices
grandes, packs/deltas y cancelación. Aún no sustituir la ruta productiva.

**G1 — Sustitución completa de lectura M2.** Adaptar modelos, controladores y ambas
interfaces, retirar la ruta de proceso y migrar metadatos. No dejar un fallback
silencioso a `git.exe` para aparentar compatibilidad.

**G2 — Escrituras y red en M5.** Admitir stage/unstage, objetos/refs y commit mediante
operaciones validadas; revisar autenticación, transporte, push, cambios de rama y
merge/rebase contra la versión entonces elegida. Las piezas de bajo nivel de una
biblioteca no equivalen a un flujo de producto completo ni a recuperación segura.

## Criterios para aceptar G1

- Distribución y prueba con PATH sin Git, sin modificar/desinstalar el Git del equipo.
  Verificar que CIDE no busca ni crea un proceso `git.exe` o auxiliares inesperados.
- Fixtures locales: repositorio normal/worktree, sin primer commit, detached HEAD,
  objetos loose/pack, cambios staged/unstaged, borrados, conflictos y no rastreados.
- Comparación con el proveedor anterior en pruebas: nombres Unicode, mayúsculas
  Windows, CRLF, bytes legacy/inválidos, ignorados, atributos y rutas no representables.
- SHA-256, formatos de refs/índice, sparse/split y repositorios parciales son casos
  explícitos: demostrar soporte o fallar con diagnóstico; nunca mostrar falso limpio.
- Revisión/generación obsoleta descartada; cancelación, UI fluida y cuotas medidas.
- Index/config/refs/objetos intactos después de lecturas, guardados seguros y
  recuperación de borradores v1/v2/v3 conservada. Fixtures y depósitos propios.

No se han ejecutado prototipos ni benchmarks del motor embebido en esta revisión.
No cambian dependencias, binarios ni la evidencia de pruebas ya cerradas.

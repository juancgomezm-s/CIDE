Actualización 2026-11-06: `jc_git::blame` ofrece atribución acotada por primer padre reutilizando el único modelo de diff. [Contrato](G3_BLAME.md). La integración comprobada es GUI/TUI de CIDE; no se declaran integrados otros hosts ni una ABI estable.

Actualización 2026-11-06: `jc_git::path_history` publica historial por ruta paginado y cancelable, sin depender del editor. [Contrato](G3_PATH_HISTORY_OUTLINE.md). Los adaptadores GUI/TUI usan el mismo controlador y diff; no se afirma integración nueva con OmniCore/OmniCoder ni una ABI estable.

# Núcleo Git reutilizable: CIDE, OmniCore y OmniCoder

Decisión aceptada por Juan: el módulo Git se integra en CIDE y conserva un núcleo
Rust independiente, reutilizable por OmniCore y OmniCoder. No es un segundo editor
ni una aplicación Git obligatoria para el usuario.

## Separación de responsabilidades

```text
                  Núcleo Git independiente en Rust
          repositorios · objetos · refs · consultas · operaciones
                    /              |               \
            Adaptador CIDE   Adaptador OmniCore   Adaptador OmniCoder
              GUI / TUI        interfaz propia      interfaz propia
```

| Parte | Responsabilidad |
|---|---|
| Núcleo Git | Lectura de objetos/índice/refs/worktree, historial, comparaciones Git y futuras operaciones de repositorio; identidades, capacidades, cuotas, cancelación, progreso y recuperación Git |
| Adaptador de aplicación | Proveer snapshots de buffers, coordinar tareas con el host, comprobar vigencia de documentos, traducir errores/mensajes, credenciales mediante un contrato explícito y confirmar acciones del usuario |
| Interfaz | Layout, tema, menús, atajos, foco, ratón, grafo dibujado, paneles y presentación del diff |

El núcleo no conoce Workbench, DocumentId/ViewId, controles Win32, celdas de terminal,
ventanas, tema, portapapeles ni configuración privada de ninguna de las tres apps.
No carga perfiles Progress ni depende del núcleo de edición. Los modelos de grafo
y comparación pueden compartirse como datos; los dibujos y recursos pertenecen a
cada frontend. Una biblioteca de presentación reutilizable, si llega a hacer falta,
será opcional y separada del motor.

## Punto de partida real

Actualización: [consulta histórica implementada](G1_HISTORICAL_READS.md). El modelo
de comparación ya reside en jc-git, con una reexportación compatible desde jc-app
y una prueba de consumo público independiente del editor.

`crates/jc-git` ya es una biblioteca Rust independiente de los crates del editor:
su manifiesto declara gix 0.90.0 y el crate interno std-only jc-fs para
compartir lecturas confinadas, sin depender de los crates del editor. Sus operaciones
actuales son lectura de blobs y estado, con metadatos de historial acotado.
`jc-app::git_read`, los controladores y el workspace GUI son consumidores.

Esta separación existente es la base, no una afirmación de API pública estable
ni de integración ya probada con OmniCore/OmniCoder. Los tipos públicos todavía
son iniciales; algunos identificadores validados y modelos de comparación viven
en jc-app, y el motor incluye mensajes Display en español. Su revisión forma parte
de estabilizar el contrato. No se duplica el código para extraerlo más adelante.

Las [comparaciones locales](G1_LOCAL_SOURCE_PAIRS.md) añaden HEAD ↔ índice e
índice ↔ disco con presencia, tokens y bytes originales. Los buffers continúan
siendo responsabilidad del adaptador; no hay un Document dentro del motor.

## Contrato del núcleo

- API con solicitudes y resultados tipados, versiones/capacidades explícitas y
  errores estructurados. Las apps convierten esos errores a mensajes para sus UI.
- Fuentes Git y fuentes suministradas por el host se distinguen explícitamente.
  Un buffer llega mediante snapshot/rango acotado y token opaco de revisión del
  host; nunca mediante un préstamo mutable a su documento o una dependencia al editor.
- Preservar bytes, rutas opacas, OID, modos y ausencia de archivo. Codificación y
  coordenadas del editor se convierten en el adaptador; no se presupone UTF-8.
- Consultas cancelables, paginación, progreso y presupuestos explícitos. El host
  controla cuándo ejecutarlas y cómo recibe resultados. No abrir un pool permanente
  o un indexador por cada consumidor sin un requisito y medición concretos.
- Una comparación Git no depende del layout. HEAD/índice/disco/revisión/buffer
  conservan identidad; el adaptador rechaza una entrega obsoleta antes de aplicarla
  al estado de su aplicación.
- Las futuras mutaciones son planes con precondiciones, alcance y publicación
  recuperable, no efectos de navegar por un panel. Una operación Git tiene su
  recuperación propia; no reutiliza el historial de undo del editor.
- El bloqueo entre procesos debe funcionar aunque dos apps abran el mismo repo.
  Una exclusión en memoria no basta. Respetar locks Git y cambios externos; nunca
  suponer que CIDE es el único escritor.
- El núcleo no incluye credenciales en logs, argv ni sesiones de editor. El host
  entrega la autorización solicitada mediante un contrato específico. No aparece
  una dependencia implícita a git.exe/ssh.exe o auxiliares externos.

El primer contrato de escritura publica cambios del índice a partir de un plan
revisado. El grafo de ascendencia y el parche unificado también están disponibles
en la biblioteca. [Fundamentos Git avanzados](GIT_ADVANCED_FOUNDATIONS.md)

## Consumo y distribución

Primera interfaz: biblioteca Rust. Cada aplicación incorpora una versión fijada
del mismo núcleo; no requiere instalar un servicio ni un Git independiente.
No habrá tres forks ni una copia de la lógica de repositorio en cada adaptador.

Antes de construir bindings se inspeccionarán los hosts reales de OmniCore y
OmniCoder. Para un consumidor .NET u otro lenguaje puede añadirse una fachada
C ABI versionada, con handles opacos, buffers longitud+bytes, liberación explícita,
cancelación y traducción de errores; nunca tipos Rust cruzando la ABI ni panics
propagados al host. Esa fachada no contiene otra implementación del motor.
No se da por hecho el lenguaje o versión actual de esos productos.

Por ahora se conserva el nombre técnico `jc-git` y la ubicación dentro del
workspace. No hace falta renombrar crates, crear un repositorio o publicar paquetes
para fijar esta independencia. Tras estabilizar el contrato y probar un segundo
consumidor podrá separarse su distribución/versionado manteniendo una sola fuente.
Las licencias y avisos de las dependencias acompañan a cada distribución.

## Efecto sobre el plan

1. **Cierre M2:** conservar lectura existente, corregir pares de comparación,
   consulta básica de una revisión/archivos y pulido de navegación/scroll. Al
   ampliar el núcleo, hacerlo sin dependencias a CIDE. No exigir bindings ni
   integración de las otras apps para cerrar este hito.
2. **Expansión Git de consulta:** completar G1–G3 como línea propia: refs/DAG,
   layouts, histórico, anotaciones y parches. Compartir los modelos y operaciones;
   adaptar las UI. No todo ese alcance bloquea M2.
3. **M5:** G4–G6 para escrituras, integración y red sobre el mismo núcleo. Fijar
   contratos y recuperación antes de exponer las acciones en cualquier app.
4. **Reutilización:** probar un consumidor de contrato independiente de jc-app,
   después el adaptador del siguiente host real. Esto prueba independencia; que
   un crate compile dentro del workspace no demuestra por sí solo integración.

Validar cada bloque con fixtures propios: repositorio/bytes intactos en lecturas,
cancelación, cuotas, obsolescencia y, cuando exista escritura, locks entre procesos,
fallos de publicación y recuperación. Las pruebas visuales de cada host son propias.
Este documento fija arquitectura; no incorpora bindings ni cambia el editor.

# Explorer compartido TUI/GUI: contrato previo

**Seguimiento:** implementado y validado localmente en
[M2B_SHARED_EXPLORER](M2B_SHARED_EXPLORER.md). Lo siguiente conserva el inventario
y propuesta anteriores a esa migración; sus hallazgos de código viejo son históricos.

**Contrato propuesto; no implementación ni aceptación ya obtenidas.** Solo lectura
estática, sin checks/runtime. Aplican [arquitectura](ARQUITECTURA.md),
[auditoría ecode](AUDITORIA_ECODE_ARQUITECTURA.md) y [F3](F3_PRESENTATION_TASKS.md).
Sublime Text conserva la referencia visual; no se porta código externo aquí.

## Hallazgos concretos

- [Explorer TUI](../crates/jc-tui/src/app/explorer.rs) enumera/inspecciona/ordena todos
  los hijos síncronamente: sin cuotas, deadline, cancelación ni generación. Es un
  navegador de un directorio, no árbol. Guarda paths separados, pero labels lossy.
- [Routing TUI](../crates/jc-tui/src/app/events.rs): Enter usa `open_path`, side usa
  `open_file_to_side`. `activate` canonicaliza/comprueba raíz, pero esas aperturas
  no usan el lector confinado compartido. Migrar ambas rutas juntas.
- [Quick Open](../crates/jc-app/src/quick_open.rs) ya tiene worker lazy, Wakeup,
  cancelación/generación/aceptación. Default: query256B, path4096 unidades, depth32,
  50 000 entradas,512 resultados,512KiB lógicos,2s. Busca recursivamente; no es árbol.
  Excluye `.git` y registra nombres no Unicode omitidos como candidatos.
- [Quick Open GUI](../crates/jc-gui/src/windows/selector_flow.rs) abre mediante
  `open_project_file(...,side,16MiB)`; [jc-files](../crates/jc-files/src/scoped_read.rs)
  valida el handle de payload. [Paint GUI](../crates/jc-gui/src/windows/paint.rs)
  aún calcula áreas de editor/split, sin una sidebar Explorer persistente.

## Catálogo, identidad y API

Un `jc-app::ProjectExplorer` por sesión, compartido como implementación TUI/GUI;
procesos distintos no comparten memoria. Mostrar lista raíz; expandir lista solo
hijos inmediatos. Sin watcher, contenido, recursión anticipada ni segundo finder.
Quick Open conserva búsqueda por query; no arrancarlo para alimentar el árbol.

NodeId opaco Copy = generación raíz + ordinal monotónico no reutilizado. Nodo:
parentID, clase, path relativo privado/opaco, expanded y estado
Unloaded/Queued/Loading/Ready/Limited/Cancelled/Failed. Sin espejo absoluto ni label
permanente. Refresh avanza revisión de directorio e invalida hijos/descendientes;
hijos nuevos reciben IDs nuevos. Overflow falla, nunca wrap. ExplorerView posee
selección por ID y scroll; tras colapso selecciona ancestro, raíz nueva reinicia.
Conservar OsString/PathBuf sin lossy: labels ASCII (`\\`, `\xNN`, `\u{HEX}`) con
escapes atómicos; nunca reconstruir paths desde etiquetas.

```text
ProjectExplorer::new(Wakeup, ExplorerLimits) -> Result<Self, ExplorerError>
set_root(PathBuf); expand(NodeId); collapse(NodeId); refresh(NodeId); cancel(NodeId)
clear(); generation(); ready(); take_recovery()
poll(current_root: &Path, max_deliveries: usize) -> bool
nodes()/visible_nodes() -> acceso/iteración prestada, no Vec clonado
open_file(NodeId, &mut Workbench, Option<TextEncoding>, to_side, max_bytes)
    -> Result<TabId, ExplorerOpenError>
ExplorerView::prepare(&ProjectExplorer, max_rows, max_cols) -> ExplorerPresentation
```

Son APIs propuestas. `set_root` admite metadata absoluta; worker verifica raíz,
tipo/canonicalización y entrega raíz+generación; owner concilia antes de exponer.
`prepare` solo normaliza viewport: sin I/O/poll/expansión/admisión. Filas llevan
NodeId/depth/clase/expanded/loadstate/selected y label acotado. Hit target GUI:
generación+revisión catálogo+NodeId; rechazar clicks de frames invalidados.

## Cuotas iniciales, pendientes de medir

| Recurso | Cota/política |
| --- | --- |
| Worker/cola | Uno lazy;1 activa,8 pending/128KiB; misma carpeta coalesce última revisión; Busy sin fingir carga |
| Completion/scratch | Una completion512KiB y scratch512KiB; esperar consumo/cancelación antes de otra entrega |
| Catálogo global | 4096 nodos/1MiB de capacidades heap incluyendo paths/índices; no multiplicar cuota por carpeta |
| Enumeración | 2048 hijos retenidos,50 000 examinados,2s,path4096 unidades,depth32 |
| Expansión/frame | 32 directorios expandidos; evictar descendientes colapsados; frame<=128 filas x256 columnas |
| Owner | <=2 completions/slot; Wakeup coalescido+ready/recovery, jamás aceptar durante paint |

Directorios primero y orden nativo estable de nombre. Limited ordena solo subset
retenido, sin prometer primer bloque lexicográfico global ni paginación completa.
Excluir `.git` explícitamente; no heredar otras exclusiones S1 silenciosamente.
Colapso cancela subárbol/invalida revisiones. Raíz nueva cancela/vacía cola+catálogo.
Drop despierta y join, sin detached; panic terminal acotado. Llamadas OS bloqueadas
pueden retrasar cancel/join/deadline. Cuotas lógicas no son garantías RAM privada;
buffers OS y reservas/staging transitorios deben declararse aparte.

## Confinamiento, apertura y routing

Excluir links/reparse, `..`, dispositivos y destinos fuera de raíz al enumerar y
admitir. Canonicalización+read_dir no prueba aislamiento metadata atómico frente a
namespace cambiante: enumeración estricta Windows requiere handle de directorio
no-reparse verificado dentro de raíz y enumeración anclada a él, reutilizando
primitivas jc-files. Resolver al implementar; si se mantiene enumeración por path,
registrar la ventana sin afirmar aislamiento atómico. Apertura de payload conserva
la validación por handle existente, independientemente del display del catálogo.

Validar ID/generación/clase y delegar `open_project_file(root,path,encoding,side,16MiB)`.
No preleer/fabricar fingerprint: árbol no promete versión; S1 conserva su apertura
versionada. Reutilizar dirty DocumentId/undo, pin, side e historial de Workbench;
fallos no agregan tabs/nav, encoding usa elección existente, nunca clonar sesiones.
ViewExplorer/CtrlShiftE ya existen. Enter directorio expande/colapsa; Right hijo;
Left/Backspace colapsa/padre; flechas/Home/End/Page recorren; F5 refresca directorio
seleccionado/padre; Enter archivo abre, CtrlEnter side. Click selecciona, disclosure
expande, dobleclick abre. Sidebar colapsable y editor dominante; cliente compacto
la oculta explícitamente, sin cubrir source ni alterar split F1.

## Propiedad y cierre del siguiente bloque

| Frente | Archivos/responsabilidad |
| --- | --- |
| Catálogo/proveedor | Nuevos jc-app/src/explorer.rs, explorer/worker.rs; IDs/cola/cuotas/cancel, sin snapshots documentales |
| API/apertura | Integrador exports/lib/Workbench y helpers jc-files necesarios; compartir utilidades concretas sin duplicar Quick Open |
| Vista | ExplorerView compartido, selección/viewport/presentación y pruebas puras |
| GUI | Nuevos windows/explorer_panel.rs y explorer_paint.rs; integrador State/Frame/mouse/notifier/comandos/root |
| TUI | Sustituir app/explorer.rs y adaptar events/paths/presentation/render/tests; retirar proveedor síncrono y aperturas alternativas al cerrar |

Sin dependencias nuevas previstas: std, Wakeup, GDI existentes. Construir catálogo,
migración TUI y sidebar GUI antes de checks. Probar lazy/quotas/Busy, cancel/panic/
Drop/generaciones, IDs viejos, Unicode/no escalares/reparse, dirty reuse/side/pin;
composición repetida/resize/split/foco y llegada sin otra tecla. Fixtures propios
workspace/work, create_new y limpieza exacta; bitmap GDI propio de árbol expandido.
Integrador actualizará PLAN/VALIDACION al cierre. No acredita implementación, IME,
accesibilidad, DPI/latencia física ni RAM global.

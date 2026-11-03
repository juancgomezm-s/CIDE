# Recuperación periódica y retención segura en Rust

Incremento local de2026-11-03 sobre `a7227a1`. Complementa
[recuperación de sesiones](SESSION_RECOVERY.md) sin cambiar su formato de disco.
El original sólo se modifica con Guardar explícito.

## Programación y captura

GUI y TUI programan una captura tras2s de inactividad; bajo actividad continua
intentan admitirla al llegar a30s. Si un documento pendiente cambia mientras se
captura, se cancela esa generación incompleta y se vuelve a intentar. El plazo de
30s es de intento, no una garantía de publicación mientras el texto cambia sin
pausa. Fallos de disco/admisión se muestran en español y esperan10s antes de
reintentar. Después de la captura inicial, una sesión sin cambios no reescribe sus
copias. La primera captura de cada ejecución también fija su estado de interfaz.

La admisión congela metadatos de sesión/vistas y cursores del codec para las
versiones iniciales. Cada turno copia como máximo64KiB. Una identidad de documento
y una época de mutación invalidan cursores ante edición, undo/redo, guardado,
cambios de historial o reubicación de almacenamiento. No se clonan Document,
FileSession ni el historial. No se serializa otra vez el prefijo en cada turno.

Un documento ya emitido completo conserva su versión inicial aunque el usuario
continúe editándolo; los documentos pendientes todavía deben corresponder a sus
cursores iniciales. Así la generación representa el estado inicial coherente,
incluidas sus vistas. La actividad posterior programa otra captura. La identidad
también impide confundir dos modelos nuevos con iguales TabId/revisiones/épocas.

Un worker recibe únicamente bytes propios a través de una cola de un tramo. Puede
haber otro tramo pendiente en el owner y otro escribiéndose: hasta192KiB de carga
de fuente, además de metadatos y descriptores acotados. Hashes, escritura, sync,
relectura y limpieza ocurren en ese worker. La admisión recorre metadatos retenidos
y conserva sus cuotas; no es una promesa de latencia física o RAM global.

La entrega y el espacio de la cola despiertan al owner. TUI combina esos avisos con
su deadline y GUI usa mensajes propios/temporizador que se retira al quedar en
reposo. El modo sin recuperación no conserva un plazo vencido ni gira en vacío.
El cierre normal cancela y espera el worker, recupera su lease y confirma después
el estado final más reciente, incluidos cambios posteriores al último checkpoint.

## Publicación y limpieza

Cada publicación revalida la versión confirmada anterior. Si HEAD se dañó pero
BACKUP sigue válido, no arrastra el HEAD dañado sobre el único respaldo correcto.
Se conservan las garantías previas de vaciado, sincronización, verificación por
relectura y publicación atómica de HEAD/BACKUP.

La limpieza protege ambas generaciones referenciadas y verifica sus payloads.
Un HEAD/BACKUP dañado impide borrar cualquier candidato. Las versiones obsoletas
se eliminan por handles Win32 fijados y comprobados, nunca con borrado recursivo.
Se fijan ancestros/directorios y se comprueba la identidad del lease. Sólo se
admiten nombres de generación y payload conocidos; puntos de reanálisis, enlaces
duros o archivos desconocidos impiden la limpieza del lote. No toca otros slots.

Cada pase admite hasta64 generaciones y8192 archivos obsoletos. Un bloqueo aplaza
la generación; una limpieza interrumpida sólo puede afectar una versión ya
obsoleta y se puede reanudar por los archivos restantes. La guarda RAII de una
generación recién creada descarta escrituras incompletas propias, incluso antes
del primer HEAD, y comprueba que ningún control publicado la referencie antes de
eliminarla. Un crash que evita Drop puede dejar restos para un pase posterior.

Después de confirmar, normalmente quedan HEAD y BACKUP. Si hay generaciones
bloqueadas o trabajo pendiente, otro worker reintenta la limpieza tras10s, incluso
sin nueva edición, sin reconstruir documentos ni avanzar HEAD. Un fallo de limpieza
no invalida una copia confirmada. Las cuotas cuentan restos retenidos; si no cabe
otra copia, falla de forma visible conservando las anteriores.

Se mantienen las cuotas de256MiB de fuentes por documento,512MiB codificados por
sesión y8GiB de depósito por slot. No hay recolección automática de otros slots,
ni certificado de resistencia a fallos eléctricos/hardware. La integración del
editor con archivos de varios GB continúa pendiente.

## Evidencia

Los informes y registros de este bloque usan el prefijo
`docs/evidence/periodic-recovery-2026-11-03-*`. Las pruebas admiten únicamente
fixtures propios bajo `work`; no leen el depósito personal ni el clipboard.
Los recorridos nativos terminan tres procesos de cada interfaz después del
checkpoint periódico, sin ordenar salida normal, y comprueban recuperación y
retención. La prueba de reposo verifica que HEAD no se reescriba.

Pasaron20 casos nuevos: tres de cursor core, siete de retención, ocho del
controlador/captura y dos de salida con worker activo, por suites y cierres
dirigidos. Se conservaron los regresos previos del codec, frontends y errores de
escritura/proceso. Fmt, Clippy con todos los targets y release offline aprobados.
No se ejecutó una suite runtime global única ni CI remoto.

GUI y TUI completaron siete comprobaciones nativas cada una, con tres procesos
por interfaz. En GUI se verificaron bytes, dirty, undo y caret con el inspector
Rust de fixtures; TUI ejercitó deshacer/rehacer desde los menús españoles. Los
originales quedaron intactos y tras limpieza se conservaron dos generaciones.
El primer harness TUI encontró un bloqueo transitorio leyendo HEAD durante su
sustitución atómica; ahora reintenta sólo esa lectura diagnóstica antes de seguir,
sin aceptar un HEAD incompleto ni cambiar el protocolo productivo. Informe inicial
conservado en `periodic-recovery-2026-11-03-tui-initial-head-read-race.json`.

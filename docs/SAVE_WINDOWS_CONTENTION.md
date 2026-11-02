# Guardado Windows — contención y recuperación

Revisión del 2026-11-02 sobre F3 `de7b8f5`. El incidente original fue un error
1175 en force_save durante una prueba de sesión. Se reproduce el mismo código
en el probe nativo bajo esta ruta; no se ha identificado el proceso/handle que
intervino en la corrida histórica. No se atribuye a OneDrive ni a otro servicio.

## Evidencia antes y después

`save_contention`, ejemplo opt-in de jc-files, crea solo su propio directorio
temporal. Cada escenario hace 100 guardados con archivos de 10 bytes CRLF:
sin contención, con lector que comparte lectura/escritura pero deniega DELETE,
y con mapeo propio de lectura. Rust 1.85.1 MSVC, Windows del equipo, release.

En la base: 0/100 errores sin contención; 100/100 con lector persistente, código
32; 1/100 con mapeo, código **1175**. En el estado corregido: 0/100, 100/100
(32, bloqueo persistente esperado), 0/100 respectivamente. Todas las fallas
observadas conservaron bytes originales y documento dirty. El mapeo no induce
1175 de forma determinista; esos conteos no prueban quién causó el incidente
histórico ni garantizan ausencia futura de bloqueos.

## Tratamiento implementado

Se mantiene ReplaceFileW. Solo 32, 33 y 1175 pueden reintentarse: cinco llamadas
como máximo, esperas 5/10/20/40 ms (75 ms de espera total, aparte del tiempo de I/O).
Antes de cada nuevo intento se comprueba que el contenido del destino conserve
la huella capturada al iniciar el commit. Si cambió o desapareció, se informa
conflicto y no se sobrescribe esa versión. Un bloqueo persistente sigue siendo
un error y el documento no se marca guardado. No hay delete + rename de respaldo
ni reintentos ilimitados en el hilo UI.

Además, la revisión detectó una ruta de pérdida de recuperación: la limpieza
eliminaba el temporal ante cualquier error. Según el contrato de Microsoft,
1176/1177 pueden dejar nombres parcialmente movidos. Ahora producen
`FileError::SaveRecovery` con destino, ruta del temporal y error original, y se
retienen los bytes para recuperación explícita. No se reintentan ni se limpia
esa copia automáticamente.

Pruebas deterministas inyectan 1175 antes de un commit nativo exitoso, bloqueo
persistente, cambio/borrado externo durante espera y errores parciales 1176/1177
con conservación del temporal. Una prueba nativa adicional comprueba liberación
de un lector después de 25 ms y bloqueo sostenido con bytes/dirty intactos.
Son regresiones del comportamiento observable, además de los probes repetidos.

Límites: no se cambia configuración del sistema ni se fuerza liberar handles
ajenos. Los checks de huella no son un CAS del filesystem: un escritor externo
puede actuar entre comprobación y reemplazo, como en el contrato previo. El
guardado es una operación explícita/fría; la lectura adicional de huella no
entra en movimiento/render. RAM global y latencia física siguen abiertas.

Fuente del tratamiento de errores:
[ReplaceFileW — Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew).

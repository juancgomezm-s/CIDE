# CPU de búsqueda configurable al lanzar la TUI

La opción `--search-workers 1..4` elige el máximo de workers de Search Plus para
esta sesión. El valor predeterminado es 2. El total incluye el coordinador:
1 usa el coordinador como scanner sin helpers; 2 permite un helper. No cambia el
pool F3, la CPU de otros proveedores ni la decodificación o el diff independientes.

```powershell
jc-ide-tui --search-workers 1 --encoding cp1252 programa.p
jc-ide-tui --encoding utf-8 --search-workers 2 "primer archivo.p" segundo.cs
```

El ejecutor limita el total efectivo al menor entre lo solicitado y las CPUs
lógicas disponibles menos cuatro, usando resta saturada y acotando el resultado
a 1..4. Así se conserva una ruta válida de un worker en equipos pequeños; pedir
4 no obliga a crear cuatro workers ni garantiza mayor velocidad. Este límite
usa la disponibilidad que informa Rust, sin cambiar afinidad, prioridad o energía.

Se rechazan 0, 5, valores no enteros, ausencia de valor y opciones duplicadas.
Cada valor válido se escribe como uno de `1`, `2`, `3`, `4`. Un error conserva la
salida de uso existente y termina con código 2; `-h`/`--help` conserva el contrato
de ayuda. Los alias de encoding y el orden de los archivos siguen siendo los
mismos; la nueva opción puede aparecer entre ellos.

El parser guarda el límite en un contenedor privado y llama a
`jc_tui::run_with_search_workers(LaunchOptions, usize)`. `LaunchOptions` conserva
su forma pública: los llamantes existentes no necesitan añadir un campo a sus
literales. El integrador conecta esa entrada con las opciones de ProjectSearch;
no se leen ni ejecutan fuentes desde el parser.

Más CPU total puede reducir tiempo de pared y también competir con otro trabajo.
Use 1 cuando prefiera reducir esa competencia; compare la carga real antes de
subir el límite. La aplicación no inspecciona Qwen ni ajusta el número de workers
según procesos ajenos. Las capturas del probe CPU no son una garantía de velocidad
para todos los proyectos o de aceptación global de latencia/RAM.

El bloque prepara pruebas del parser sobre defaults, los cuatro valores válidos,
encodings/rutas, errores y ayuda. El trabajador aplicó formato y revisión estática
sin ejecutar Cargo ni repetir el perfil CPU; el integrador valida el frontend al
cerrar la conexión del entrypoint y las opciones de búsqueda.


## Cierre local del integrador

Integrado `run_with_search_workers` y constructor TuiApp configurable conservando
LaunchOptions. El límite pasa al draft de cada consulta y a ProjectSearchOptions;
se valida antes de entrar en terminal. Pasaron **47 pruebas de jc-tui** (42 lib,
cinco parser), Clippy all-targets de TUI y release del binario después de integrar.
El CLI release real comprobó ayuda y errores/duplicados sin abrir terminal.
Son cinco pruebas nuevas respecto al bloque CPU anterior: total 274 distintas,
no aprobación inferida de las 269 previas. No se repitieron perfiles por este flag.
CPU/memoria/costes y ausencia de pantalla física en [CPU_PERFORMANCE](CPU_PERFORMANCE.md).

# M2 · Pintado de la GUI en geometrías grandes

El búfer anterior rechazaba un cliente de 4096×2049 o 5120×2160, aun cuando
solo había que pintar una región de 64×64. La reproducción en un HWND propio
obtuvo ambas dimensiones, registró un error de cuota por intento y volvió a
pintar al reducir a 1000×650. El límite exacto de 4096×2048 sí se pintaba.

La GUI conserva un solo DIB de 32 bits y hasta 32 MiB. En el rango que cabe en
una superficie, presenta la región invalidada con un blit. Si la altura
supera la capacidad del DIB, compone bandas horizontales de esa misma región.
Cada banda usa el mismo frame y el mismo DIB, con el origen del DC desplazado
para conservar las coordenadas del cliente. Una invalidación pequeña usa una
pasada aunque atraviese una unión de la composición completa. Las filas de
texto fuera del clip se omiten antes de dibujarlas. El mismo camino sirve para
`WM_PAINT` y `WM_PRINTCLIENT`.

El destino recibe solo bandas cuya composición terminó. Un error se registra
en el contador habitual y la publicación de hits de Markdown requiere que
termine la pintura. La reserva se reutiliza entre bandas y se sustituye al
cambiar las dimensiones del búfer. El tamaño máximo lógico sigue en 8192 por
dimensión. Los contadores de composición y blit cuentan cada banda.

## Prueba oculta

El proceso se lanza con una fuente UTF-8 de 256 KiB y un HWND de prueba oculto.
El equipo limitó el primer intento a 2580×1100. El medidor amplía el máximo de
redimensionado solo en ese HWND para alcanzar los tamaños solicitados mediante
`SetWindowPos` y recorrer el `WM_SIZE` real. DPI 96, 144 y 192 se inyectan por el
mensaje correspondiente dentro del proceso. El lanzador comprueba los bytes de
la fuente y el medidor comprueba identidad, revisión y posición.

La fase de memoria usa un destino de 64×64 de 16.384 bytes. La fase de
composición completa usa un DIB del tamaño del cliente e incluye su coste en
las muestras crudas. La primera composición completa se registra aparte de las
21 muestras calientes por tamaño medido. Se comprueban colores alrededor de la
unión y en la barra de estado, además de las asignaciones y los blits.

## Resultado

Los ocho casos pasaron con el binario release. La memoria privada de la tabla
se toma al componer sobre el destino de 64×64. El p95 mide frames completos
en una serie distinta que usa su propio destino de tamaño completo.

| Cliente | DIB del editor | Memoria privada con destino de 64×64 | Blits del frame completo | p95 del frame completo |
|---|---:|---:|---:|---:|
| 3840×2160 | 31,64 MiB | 34,77 MiB | 1 | 23,35 ms |
| 4096×2048 | 32,00 MiB | 35,27 MiB | 1 | |
| 4096×2049 | 32,00 MiB | 35,27 MiB | 2 | |
| 5120×2160 | 31,99 MiB | 35,23 MiB | 2 | 24,12 ms |
| Regreso a 1000×650 | 2,48 MiB | 5,69 MiB | 1 | |

El pico bruto de 77,50 MiB se registró con el destino de medición completo de
42,19 MiB para 5120×2160. El resultado de memoria con el destino pequeño es
una muestra del proceso en una fase distinta, no una resta sobre ese pico.
Las métricas de celdas aumentaron con DPI 144 y 192 y volvieron a su valor de
referencia con DPI 96. El documento conservó sus bytes, revisión y posición.

Pasaron cuatro pruebas dirigidas del búfer, formato, Clippy GUI all-targets y
release offline del workspace. La entrega está en `target/m2-geometry-next/release`.
Los informes del rechazo anterior y la corrida aprobada incluyen muestras,
contadores, tamaños efectivos y hashes del ejecutable. [Evidencia](evidence/m2-gui-geometry-2026-11-07.json)

```powershell
python scripts/measure-m2-editor-geometry.py --exe target/m2-geometry-next/release/cide-gui.exe
```

Las mediciones corresponden al proceso oculto y al destino GDI. La revisión del
compositor, la latencia visible y el traslado físico entre monitores conservan
su propia aceptación de M2.

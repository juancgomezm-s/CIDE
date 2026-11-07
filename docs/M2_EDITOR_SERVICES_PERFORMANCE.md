# M2 · Memoria y rendimiento con servicios simultáneos

Tres procesos nuevos de CIDE GUI release midieron el mismo escenario en serie el
2026-11-06. Se usaron Ryzen 9 8945HS, Windows 10.0.26300, la caché de compilación
existente y una ventana propia oculta de 1000×650. Los picos son de compromiso
privado e incluyen los 2,48 MiB de la superficie adicional del medidor.

| Corrida | Pico privado (MiB) | Privada después de liberar (MiB) | p95 de composición (ms) | Máximo (ms) | Composiciones durante el solapamiento observado |
|---:|---:|---:|---:|---:|---:|
| 1 | 27,08 | 7,31 | 4,05 | 7,90 | 10 |
| 2 | 27,04 | 7,67 | 3,72 | 7,03 | 8 |
| 3 | 26,38 | 7,62 | 3,79 | 9,87 | 11 |

## Escenario y alcance

- 64 fuentes UTF-8 de 256 KiB en un repositorio propio. CIDE abre cuatro fuentes
  y el README con una imagen local BMP de 1024×1024. Search Plus entrega exactamente
  64 coincidencias y Git entrega tres cambios con el estado de índice y disco
  esperado. El lanzador prepara el repositorio con Git local y arranca CIDE con
  `PATH` vacío. Los hashes de las fuentes y `.git` se conservan en las tres corridas.
- Una edición en `f000.rs` provoca una captura periódica por la cola real de
  mensajes. Durante la captura se refrescan búsqueda y Git con la imagen ya
  decodificada. La traza acredita estados de controladores que coinciden, con
  recuperación activa, búsqueda en curso y Git en cola o en curso. La imagen
  aporta 4 MiB retenidos. La traza no instrumenta intervalos de ejecución de CPU
  por trabajador.
- Cada corrida compone 120 escenas nativas en un DIB del mismo proceso mediante
  `WM_PRINTCLIENT`. Las cifras incluyen preparación y dibujo. Hubo 128 muestras
  con recuperación activa y 29 con el solapamiento completo descrito. La medición
  de composición excluye compositor y pantalla física.
- Al salir de la vista Markdown, una referencia débil comprueba que se liberaron
  los píxeles. Después se limpian búsqueda y Git y se suelta el DIB del medidor.
  El contador de CPU registró 0 ms en cada ventana de reposo de unos dos segundos,
  a la resolución del sistema. El pico de working set fue de 41,71 MiB.
- Se reabre el depósito aislado después de tomar las muestras y se valida byte a
  byte la edición recuperada. Cada publicación usó 26 entregas con bloques de
  datos de hasta 65.536 bytes. La verificación usa la generación periódica
  publicada antes de la salida del modo de prueba.

## Corrección encontrada

La primera prueba en depuración completó búsqueda, Git y recuperación, pero dejó
la imagen sin cargar. En Windows, una ruta de documento recibida por CLI podía
tener un prefijo distinto al de la raíz canónica del proyecto. La vista ahora
fija la carpeta canónica del documento al abrirse y la comparte entre imágenes y
enlaces relativos. Los accesos locales siguen pasando por las comprobaciones de
confinamiento existentes. Preparar el frame usa la ruta ya resuelta.

Pasaron las siete pruebas dirigidas de medios Markdown, incluida la regresión
nueva de carga de imagen y apertura lateral de un enlace desde la ruta CLI. La
corrida funcional posterior en depuración pasó. En ella Git completó después de
la captura, por lo que sus datos quedan fuera de las tres mediciones optimizadas.
También pasaron formato, Clippy GUI all-targets y release del workspace offline.

```powershell
python scripts/measure-m2-editor-services.py --exe target/m2-services-next/release/cide-gui.exe
```

Las muestras, las fases y el hash del ejecutable están en la [evidencia de las
tres corridas](evidence/m2-editor-services-2026-11-06.json). Los informes crudos
con los inventarios de hashes permanecen en `work/m2-services-*/report.json`.
La entrega está en `target/m2-services-next/release`. Queda pendiente medir otros
tamaños de viewport y cerrar la aceptación física de M2 con escritorio y hardware
disponibles. La geometría medida y estas cuotas delimitan el resultado de 27,08 MiB.

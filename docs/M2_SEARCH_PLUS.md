# JC-IDE Search Plus — Búsqueda como ventaja principal

La búsqueda debe ser más poderosa que la del editor típico, pero ejecutarse **bajo demanda**, sin servidor remoto, AVM, LSP o indexador obligatorios.

**Orden actualizado 2026-11-01:** S0/S1 dependen de M2-F de [PLAN.md](PLAN.md). F1 corrigió replace-all para usar una transacción y mapas completos. F2 ya resuelve posiciones mediante la proyección compartida y virtualiza filas visibles del panel. El escaneo de consulta sigue síncrono y la cota/cancelación de resultados/jobs corresponde a F3/Search Plus. Ver [checkpoint F1](F1_DOCUMENT_VIEWS_TRANSACTIONS.md). La [auditoría de ecode y JC-IDE](AUDITORIA_ECODE_ARQUITECTURA.md) aporta evidencia y referencias para la sustitución.

**Avance 2026-11-02:** S1 literal implementado: worker acotado, filtros, snapshots de abiertos, lotes, cancelación, destinos versionados y reemplazo con preview/confirmación/conflictos por archivo. Uso y límites en [S1_PROJECT_SEARCH.md](S1_PROJECT_SEARCH.md). S0 sigue con escaneo síncrono y case-insensitive ASCII; no se afirma Unicode case folding completo ni regex. M2A/global RAM/latencia física siguen abiertos.

## S0 — Archivo activo: M2A

- Ctrl+F: búsqueda incremental; F3 y Shift+F3: siguiente/anterior.
- Ctrl+H: reemplazo, con undo y preservación de codificación.
- Ctrl+F3 / Ctrl+Shift+F3: buscar la selección siguiente/anterior (pendiente).
- Ctrl+G: ir a línea/columna con límites (pendiente).
- Encontrar todas las coincidencias y navegar resultados con ubicación tipada.
- Sensibilidad a mayúsculas, palabra completa, historial y marcadores.
- Regex solo con motor real limitado en tiempo/trabajo, no un switch decorativo.

**Limitación actual:** case-insensitive compara bytes ASCII. No anunciar todavía case folding completo para español, Unicode ni todas las codepages. Es un requisito expreso de la próxima implementación.

## S1 — Proyecto: prioridad alta tras S0

- Ctrl+Shift+F: escaneo recursivo con filtros glob y exclusiones, restringido al workspace.
- Streaming de resultados, cancelación, presupuesto de memoria y tamaño de archivos.
- Detectar binarios y enlaces; nunca salir de la raíz autorizada.
- Respetar encoding del proyecto/documento sin convertir legacy ambiguo.
- Resultados con archivo, línea, columna, snippet y rango de bytes.
- Replace in Files con diff agrupado, confirmación y verificación de conflictos de disco.
- Uso opcional de ripgrep cuando resulte apropiado; fallback propio.

Los resultados incluyen generación de consulta y revisión/huella de origen. Preferir snapshots de documentos abiertos sucios al disco. Workers, colas, bytes examinados y lotes tienen cuotas; el panel virtualiza filas y no decodifica todo el archivo por resultado. Cancelar/cambiar consulta descarta entregas anteriores y libera snapshots. Un reemplazo usa una transacción por documento y valida cambios de disco por archivo: no se promete atomicidad global del proyecto.

**Presupuesto inicial de diseño, no benchmark:** mostrar 1,000 resultados, máximo configurable 10,000 antes de requerir refinar; cota de bytes por archivo y cancelación inmediata. No reintentar búsquedas ilimitadamente.

## Capacidad transversal y extensiones

El motor `SearchController`, la búsqueda textual/regex, filtros y presentación de resultados pertenecen al editor. Los símbolos, referencias XREF y consultas a esquema son **proveedores semánticos de extensión**: `progress.abl` cuando hay ABL, `csharp.dotnet` para C# u otro proveedor para Rust/Python. Ningún proveedor puede reemplazar el historial, navegación o motor de búsqueda base.

Ver [EXTENSION_ARCHITECTURE.md](EXTENSION_ARCHITECTURE.md).

## S2 — Búsqueda ABL semántica: M3-M4

- PROCEDURE, FUNCTION, clases, métodos, variables y definiciones.
- Includes, RUN estáticos, consultas a tablas/campos.
- Integración XREF/XREF-XML con AVM configurada para referencias verificables.
- Distinguir resultado textual, inferencia heurística y evidencia del compilador.
- Nunca presentar como resuelta una referencia dinámica desconocida.

## S3 — Extras opcionales: M5/post-M5

- Fuzzy de símbolos, archivos, historial y marcadores.
- Índice incremental solo opt-in, con límites de RAM/disco.
- Integración opcional con OmniCore/RAG, sin dependencia para búsqueda local.

## Arquitectura mínima

- SearchController común a TUI/GUI administra filtros, presupuesto, cancelación y ciclo de vida.
- SearchText trabaja sobre archivo actual; SearchProject escanea por streaming.
- Resultados tipados son reutilizados por panel, palette y navegación.
- El frontend nunca ejecuta una búsqueda masiva bloqueante en el hilo de teclado.
- El reemplazo solo aplica a revisiones de archivo conocidas y utiliza FileSession.

## Atajos y restricciones

| Atajo | Acción |
|---|---|
| Ctrl+F | Find |
| Ctrl+H | Replace |
| F3 / Shift+F3 | Coincidencia siguiente/anterior |
| Ctrl+Shift+F | Buscar en proyecto |
| Ctrl+G | Ir a línea (reservado) |
| Ctrl+F3 / Ctrl+Shift+F3 | Buscar selección (reservado) |
| Ctrl+J | Panel inferior |
| F7 | Check Syntax ABL, nunca reemplazar por búsqueda |

Menú y Command Palette son fallback para terminales que no entregan todas las combinaciones.

## Criterios de aceptación

- Fuentes UTF-8 y legacy sin pérdida de bytes.
- Resultados precisos y navegables; no inventar ubicaciones.
- Procesamiento cancelable y acotado por consulta.
- Sin escaneo o indexación permanente por defecto.
- Preview y confirmación antes del reemplazo masivo.
- Fallos de lectura por archivo visibles, sin abortar toda la búsqueda.

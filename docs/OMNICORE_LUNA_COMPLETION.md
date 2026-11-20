# Autocompletado de CIDE con GPT‑6 Luna

El 20 de noviembre de 2026 se ejecutaron seis solicitudes de autocompletado con archivos sintéticos en Windows x64. Cada solicitud usó el puente nativo de CIDE y el perfil `gpt-6-luna` de OmniCore con el proveedor ChatGPT. Se lanzó un proceso por caso, en serie y sin reintentos, con un límite de 30 s.

| Lenguaje | Caso | Tiempo | Resultado |
|---|---|---:|---|
| C# | Completar un miembro | 1,372 s | Salida con código 1, sin diagnóstico clasificado |
| C# | Cerrar una llamada | 5,611 s | Insertó `)` |
| Rust | Completar una llamada | 5,128 s | Insertó `d` |
| Rust | Completar una expresión | 9,188 s | Insertó `value * 2` |
| TypeScript | Completar un miembro | 5,166 s | Insertó `p` |
| TypeScript | Completar una expresión | 5,680 s | Insertó `Number.isNaN(parsed) ? 0 : parsed` |

Las cinco respuestas válidas respetaron el formato de inserción y produjeron el resultado esperado. Se insertó cada propuesta en su archivo y se comprobó con Roslyn, `rustc` 1.88 o `tsc`, según el lenguaje. La mediana de las respuestas válidas fue de 5,611 s y el rango fue de 5,128 a 9,188 s. Ninguna solicitud agotó el límite.

El tiempo incluye el arranque del puente y la ida y vuelta al proveedor. Quedan fuera la espera automática de 650 ms, el pintado del texto fantasma y la compilación posterior a la inserción. La muestra mide seis casos concretos.

# Desarrollo

CIDE está en M2A: la TUI está en construcción sobre el núcleo de edición, archivos y modelo de interacción compartido.
El objetivo es llegar a un editor ligero para Windows con varios lenguajes y una integración ABL útil.

## Preparar el entorno

Instala Rust desde [rustup.rs](https://rustup.rs/). En Windows el destino MSVC requiere
las herramientas de compilación de Microsoft correspondientes. El archivo
`rust-toolchain.toml` fija Rust 1.85.1 para reproducir esta base.

```console
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo run -p jc-cli --offline -- demo
```

El workspace puede usar dependencias externas seleccionadas cuando resuelven un requisito concreto; la TUI, por ejemplo, usa crossterm. La instalación del toolchain y la resolución inicial de dependencias pueden requerir red.

GitHub Actions está **diferido hasta M5**. Durante M2–M4 no se ejecuta automáticamente en push/PR ni se usa como bucle de depuración. El workflow permanece manual y dormido. Las comprobaciones anteriores son para ejecución local selectiva; si no hay toolchain disponible, documenta la validación pendiente en lugar de consumir CI remoto.

## Distribución del código

| Área | Responsabilidad |
|---|---|
| `jc-core` | Bytes, fragmentos, operaciones de edición, historial y regiones |
| `jc-languages` | Perfiles y detección por nombre/extensión; sin motores de sintaxis todavía |
| `jc-progress` | Contrato de solicitudes, argumentos del proceso y lectura de respuestas |
| `jc-app` | Comandos, foco, menús, pestañas y estado compartido por interfaces |
| `jc-tui` | Frontend terminal keyboard-first en desarrollo |
| `jc-cli` | Comprobaciones reproducibles del prototipo, sin GUI |
| `progress/abl` | Auxiliares originales ABL que deben probarse con la instalación objetivo |

## Cambios y evidencia

Explica el problema, el comportamiento resultante y cómo lo comprobaste. Al tocar el
buffer, conserva casos con bytes no UTF-8, cambios de finales de línea, selección invertida
y deshacer/rehacer. Al tocar el puente, comprueba rutas con espacios, delimitadores y
respuestas truncadas. Los errores deben ser observables y no convertirse en éxitos vacíos.

Usa archivos de ejemplo sintéticos. Las configuraciones reales de conexión y los fuentes
del ERP permanecen fuera del repositorio. Las opciones `.local.pf`, `.local.ini` y
`.local.toml` están ignoradas para separar la configuración de la máquina.

Consulta [LICENSING.md](LICENSING.md) antes de incorporar código de otros proyectos.

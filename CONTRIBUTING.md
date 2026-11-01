# Desarrollo

JC-IDE está en M0: biblioteca de edición y herramientas de validación sin interfaz gráfica.
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

Los paquetes del workspace no descargan crates externos. La instalación inicial del
toolchain sí requiere descargar Rust. GitHub Actions está preparado para Windows y Linux;
la disponibilidad de un workflow no demuestra que ya se haya ejecutado.

## Distribución del código

| Área | Responsabilidad |
|---|---|
| `jc-core` | Bytes, fragmentos, operaciones de edición, historial y regiones |
| `jc-languages` | Perfiles y detección por nombre/extensión; sin motores de sintaxis todavía |
| `jc-progress` | Contrato de solicitudes, argumentos del proceso y lectura de respuestas |
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

# Validacion local, sin CI al hacer push

Politica vigente desde 2026-11-09, tambien en M5: las comprobaciones se hacen
en el equipo local. Push y pull request no disparan pruebas de GitHub Actions
ni autorizan a los agentes a pedir, despachar, reintentar o habilitar CI remota.
Esta politica sustituye las menciones historicas de CI diferida hasta M5.

## Comando equivalente

Desde el repositorio, con Python 3.11+ y Rust 1.88.0 ya instalados:

```console
python scripts/check-local.py
```

Ejecuta en serie y se detiene en el primer fallo:

1. `cargo fmt --all -- --check`
2. `cargo test --workspace --locked --offline`
3. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
4. `cargo build --workspace --release --locked --offline`
5. `cargo run --locked --offline -p jc-cli -- demo`

Requiere rustfmt, Clippy, linker de la plataforma y dependencias ya presentes
en la cache. Busca primero el rustup aislado de `work/toolchain` en Windows;
en otro caso usa el rustup instalado en PATH. Resuelve el Cargo del toolchain
fijado en `rust-toolchain.toml`, sin descargarlo ni instalar componentes.
No hace `cargo fetch`, no modifica Cargo.lock y no llama GitHub, git push,
servicios, runners persistentes ni hooks. No cambia la configuracion del sistema.
Si faltan requisitos, informa del bloqueo; nunca usa CI remota como sustituto.
Preparar dependencias inicialmente puede requerir red y debe hacerse aparte.

Usa dos jobs de compilacion por defecto (`--jobs 1` a `--jobs 4`). Reutiliza
`target/embedded-git-next` para comprobaciones debug y `target/build-cache`
para release. Usa fixtures temporales en `work/test-temp`. No borra caches ni
exporta/sobrescribe entregas anteriores. Para una nueva entrega inmutable sigue
usando `scripts/build-cide-local.py --output target/<nueva-entrega>/release`.

## Comprobaciones selectivas

```console
python scripts/check-local.py --dry-run
python scripts/check-local.py --steps fmt demo
python scripts/test-check-local.py
```

`--dry-run` solo muestra comandos; no es una validacion. Los pasos selectivos
no equivalen a una suite completa. No repitas suites historicas por cambios
solo documentales. Si un paso falla, reporta el fallo y los pasos no ejecutados.
Las pruebas nativas GUI/ConPTY, OpenEdge, aceptacion fisica y mediciones siguen
sus guias propias; no quedan certificadas por este comando. Una ejecucion en
Windows no acredita Linux: ejecuta el mismo comando en un Linux local preparado
cuando haga falta cobertura de ambas plataformas, sin descargar ni instalar
una VM/WSL automaticamente.

## Excepcion remota y publicacion

`.github/workflows/ci.yml` conserva exclusivamente `workflow_dispatch` y la
matriz Windows/Linux para una peticion explicita y separada del propietario
para esa ejecucion concreta. Ni llegar a M5 ni publicar un commit da ese permiso.
La via manual prepara su cache y llama el mismo ejecutor; no se ejecuto como
parte de este traslado. No hay workflows de release/deploy en este checkout.

Estos cambios locales solo afectan al remoto cuando se publiquen de forma
autorizada. Antes de publicar, revisa el diff remoto exacto y los checks requeridos
por protecciones de rama. Si exigen CI, reporta el bloqueo y pide una decision;
no cambies protecciones, no falsifiques estados ni dispares CI para sortearlo.
No se ha creado un hook pre-push: la validacion se invoca de forma explicita
al cerrar un bloque y se registra que pasos y plataforma se comprobaron.

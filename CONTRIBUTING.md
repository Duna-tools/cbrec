# Contribuir a cbrec

Trabajamos con dos ramas permanentes: `main` (estable) y `beta` (integración).
Crea ramas cortas desde `beta`, por ejemplo `duna/fix-recuperacion-grabaciones`,
y abre un pull request hacia `beta`. Los parches urgentes para estable se revisan
contra `main` y después se incorporan también a `beta`.

Antes de enviar un cambio:

Localiza el área en [MAINTAINERS.md](MAINTAINERS.md) y consulta el
[mapa del código](docs/architecture.md). Si el cambio cruza áreas, revisa sus
consumidores y pruebas. Separa una reorganización de una corrección funcional
cuando ambas puedan revisarse y verificarse por separado.

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets --all-features
python3 scripts/check_release.py --self-test
```

Con FFmpeg y ffprobe instalados, ejecuta también `cargo test --locked -- --include-ignored`.
Para cambios del panel, usa `python3 scripts/test_tui_pty.py target/debug/cbrec`
después de `cargo build --locked`. La QA de píxeles requiere las herramientas
indicadas en [miniaturas](docs/thumbnail_rendering.md).

Explica el defecto, el cambio y las pruebas reales. Conserva los contratos y las
grabaciones; incluye una regresión cuando corrijas un fallo. Mantén los commits
cohesivos: asunto breve en inglés técnico y cuerpo en español. Documentación,
comentarios propios y mensajes humanos nuevos van en español; identificadores
de aplicación, APIs e infraestructura, en inglés.

No adjuntes cookies, sesiones, archivos de configuración personal, grabaciones,
rutas de tu equipo ni logs sin revisar. Puedes usar un seudónimo y el correo
noreply de GitHub para futuros commits; revisa autor y committer antes del push.
Ese correo oculta la dirección personal, pero conserva atribución a la cuenta.

Consulta el [flujo de ramas y releases](docs/development.md). Los problemas de
seguridad se notifican mediante el [canal privado](SECURITY.md).

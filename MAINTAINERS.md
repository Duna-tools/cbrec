# Áreas de mantenimiento

La identidad pública de mantenimiento es **Duna-tools**. Los cambios se coordinan
mediante issues y pull requests del repositorio; las vulnerabilidades siguen
[SECURITY.md](SECURITY.md). Esta lista orienta la revisión, no asigna aprobadores
automáticos ni representa una revisión independiente.

| Área | Código y archivos asociados | Verificación relevante |
|---|---|---|
| Dominio y contratos | `src/domain/`, `tests/` | Tipos validados, argumentos y comportamiento CLI |
| Grabación y vigilancia | `src/application/recording.rs`, `src/application/watch_service.rs`, `src/application/commands/record.rs`, `src/infrastructure/external/` | Cancelación, conservación de archivos, fallos HTTP y FFmpeg real |
| Descubrimiento, TUI y miniaturas | `src/application/commands/discover.rs`, `src/application/commands/tui.rs`, `src/application/thumbnail.rs`, `src/presentation/`, `examples/`, `scripts/test_tui_*.py` | Paginación, entradas, PTY, restauración del terminal y QA gráfica |
| Configuración y sesión | `src/infrastructure/config/`, `src/infrastructure/browser_session.rs`, `src/application/cli_controller.rs`, `src/application/utils.rs`, `scripts/cbrec-violentmonkey.user.js` | Validación, compatibilidad, transferencia local y ausencia de secretos en errores |
| Integración y distribución | `.github/`, `Cargo.toml`, `Cargo.lock`, `scripts/check_release.py`, `scripts/test_check_release.py` | CI, metadatos, tags, inventario y paquetes |

Una modificación puede afectar varias áreas: el cliente HTTP también sirve al
descubrimiento y las miniaturas. Revisa todos sus consumidores antes de cambiarlo.
Los comandos se encuentran en [CONTRIBUTING.md](CONTRIBUTING.md) y los límites de
las capas, en [arquitectura](docs/architecture.md).

La referencia es el [mapa de mantenedores de Linux](https://github.com/torvalds/linux/blob/master/MAINTAINERS):
relacionar responsabilidades con archivos, adaptado al tamaño y al seudónimo del proyecto.

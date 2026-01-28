# CHANGELOG

## [Unreleased]
### Agregado
- Capa de aplicacion para orquestar la CLI.
- Soporte de grabacion concurrente con limite de 3.
- Estructura de salida cb_rec/<modelo>.
- Opcion `--jobs` para controlar la concurrencia.
- Tests de CLI para multiples modelos.
- Opcion `--ffmpeg-path` para rutas personalizadas de ffmpeg.
- Cancelacion global con Ctrl+C sin polling activo.
- Carga de configuracion desde `config/default.toml` si existe.
- Deduplicacion de modelos de entrada.
- Cola de trabajo con workers para limitar tareas simultaneas.
- Logs compactos en ejecucion concurrente.
- Reintentos con backoff ante 429/5xx en API.
- Menor ruido de ffmpeg con `-loglevel error`.
- Seleccion dinamica de variantes HLS por resolucion con fallback.
- Flags cortos `-c` (check) y `-l` (list) para uso simple.
- CI genera paquetes `.deb` y `.rpm` para Linux.
- CI genera artifacts Linux para aarch64.
- Paquete Arch Linux (`.pkg.tar.zst`) generado en CI.

### Modificado
- CLI admite multiples modelos en modo principal y en `record`.
- Configuracion de salida usa una raiz y genera `cb_rec/<modelo>`.
- Configuracion de ejemplo usa `output_root`.
- Resolucion de rutas por defecto usando carpetas del sistema.
- Ejecucion de ffmpeg con rutas no UTF-8.
- Separacion de errores de dominio e infraestructura.
- Parser HLS usa quick-m3u8 para extraer variantes de calidad.
- Formateo y mensajes de salida se movieron a la capa de presentacion.

### Eliminado
- N/A

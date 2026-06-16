# cbrec

Simple, efficient, low-resource stream recorder written in Rust.

Grabador de streams simple, eficiente y de bajo consumo, escrito en Rust.

## Language / Idioma

- [Español](#español)
- [English](#english)
- [Releases](https://github.com/Duna-tools/cbrec/releases)
- [Threat Model](docs/threat_model.md)
- [License](#license--licencia)

---

## Español

### Descripción

**cbrec** permite grabar streams de Chaturbate de forma sencilla. Proporciona el nombre del modelo y comienza a grabar inmediatamente. Con el comando `watch` puedes monitorizar varios modelos en segundo plano y grabar automáticamente cuando se conecten.

### Características

| Característica | Descripción |
|---|---|
| Grabación directa | Sin configuración previa, arranca al instante |
| Daemon `watch` | Monitoriza modelos y graba automáticamente al conectarse |
| Autenticación | Soporte de cookie de sesión para rooms privados/fan-only |
| Polling adaptativo | Intervalo corto con actividad, largo cuando todo está offline |
| Modo confirmación | `--ask` para pedir permiso antes de grabar cada modelo |
| Grabaciones paralelas | Hasta N modelos simultáneos configurable |
| Clips cortos | `--duration SECS` para grabaciones temporizadas |
| FFmpeg flexible | PATH, `--ffmpeg-path`, `CBREC_FFMPEG` o paquete Windows con FFmpeg incluido |
| Shutdown limpio | Ctrl+C detiene grabaciones activas correctamente |
| Bajo consumo | ~3-5 MB RAM en reposo, 0% CPU durmiendo entre ciclos |
| DDD/Onion | Arquitectura modular y extensible |

### Instalación

#### Binarios precompilados

Descarga el binario para tu plataforma desde la [página de releases](https://github.com/Duna-tools/cbrec/releases):

| Plataforma | Archivo |
|---|---|
| Linux x86_64 | `cbrec_linux_amd64.tar.gz` |
| Linux x86_64 (Debian/Ubuntu) | `cbrec_*.deb` |
| Linux x86_64 (Fedora/RHEL) | `cbrec_*.rpm` |
| Linux (Arch) | `PKGBUILD` |
| macOS x86_64 | `cbrec_macos_x86_64.tar.gz` |
| macOS Apple Silicon | `cbrec_macos_aarch64.tar.gz` |
| Windows x86_64 | `cbrec_*.msi`, `cbrec_*_portable.zip` o `cbrec.exe` |

Para Arch Linux:

```bash
makepkg -si
```

#### Compilar desde fuentes

Requisitos: Rust 1.70+ y FFmpeg disponible.

```bash
git clone https://github.com/Duna-tools/cbrec.git
cd cbrec
cargo build --release
```

El binario queda en `target/release/cbrec`.

### Uso

#### Grabación directa

```bash
# Grabar un modelo con detección automática de calidad
cbrec nombremodelo

# Grabar varios modelos en paralelo
cbrec alice bob charlie

# Usar un directorio de salida personalizado
cbrec nombremodelo -o ~/mis_videos

# Elegir calidad de video
cbrec nombremodelo -q 720p

# Grabar un clip corto de 20 segundos
cbrec --duration 20 nombremodelo

# Usar una ruta personalizada a FFmpeg
cbrec nombremodelo --ffmpeg-path /usr/local/bin/ffmpeg

# Usar FFmpeg desde variable de entorno
CBREC_FFMPEG=/usr/local/bin/ffmpeg cbrec nombremodelo
```

La calidad por defecto es `best`: cbrec resuelve la variante de mayor resolución disponible y graba esa URL directa. Si usas `--duration`, el archivo se trata como clip explícito y no se marca como "archivo muy pequeño" por el umbral normal de grabaciones largas.

Puedes pasar nombres de modelo o URLs de Chaturbate; cbrec normaliza ambos al mismo nombre interno antes de grabar, monitorizar o guardar en la lista.

#### Daemon de monitorización: `watch`

```bash
# Monitorizar y grabar automáticamente
cbrec watch alice bob charlie

# Pedir confirmación antes de grabar cada modelo
cbrec watch alice bob --ask

# Usar directorio y calidad personalizados
cbrec watch alice bob -o ~/grabaciones -q 1080p

# Usar cookie de sesión
cbrec watch alice bob --session-cookie "PHPSESSID=abc123; chaturbatesid=xyz"
```

Ejemplo de salida:

```text
=== cbrec watch iniciado ===
Monitorizando: alice, bob
Presiona Ctrl+C para detener

[14:30:01][alice] offline
[14:30:01][bob] offline
[14:30:01] Próximo ciclo en 60 s
[14:31:01][alice] ONLINE detectado
[14:31:01][alice] Iniciando grabacion...
```

Con `--ask`:

```text
[14:31:01][alice] ONLINE detectado
[14:31:01] ¿Grabar a alice ahora? [Y/n]: y
[14:31:02][alice] Iniciando grabacion...
```

#### Comandos auxiliares

```bash
# Verificar si un modelo está online
cbrec check nombremodelo

# Grabar explícitamente
cbrec record alice bob

# Grabar explícitamente un clip de 20 segundos
cbrec record alice --duration 20

# Listar calidades disponibles
cbrec alice -l

# Revisar FFmpeg, configuracion, salida y lista watch
cbrec doctor

# Ver ayuda
cbrec --help
cbrec watch --help
```

### Autenticación

Para acceder a rooms privados o fan-only, o para reducir bloqueos durante el polling, puedes usar una cookie de sesión.

Opción recomendada, archivo de configuración:

```toml
# ~/.config/cbrec/config.toml
[auth]
session_cookie = "PHPSESSID=valor; chaturbatesid=valor"
```

Opción para scripts o sesiones temporales:

```bash
CBREC_SESSION_COOKIE="PHPSESSID=valor; chaturbatesid=valor" cbrec watch alice
```

Opción compatible pero menos segura, flag en línea de comandos:

```bash
cbrec watch alice --session-cookie "PHPSESSID=valor; chaturbatesid=valor"
```

Evita `--session-cookie` cuando puedas: puede quedar visible en el historial del shell o en listados de procesos.

Cómo obtener la cookie:

1. Inicia sesión en `chaturbate.com` en tu navegador.
2. Abre DevTools con F12.
3. Ve a **Application** > **Cookies** > `https://chaturbate.com`.
4. Copia los valores de `PHPSESSID` y `chaturbatesid`.

La cookie funciona para `watch`, `record` y `check`.

### Configuración

Archivo: `~/.config/cbrec/config.toml`

```toml
[general]
# Directorio base donde se guardan las grabaciones.
# Se crea cb_rec/<modelo> dentro de esta ruta.
output_root = ~/Videos

# Tamaño mínimo de archivo en bytes.
# Archivos menores se mueven a /small. Debe ser mayor a 0.
min_file_size = 262144000

[naming]
# Variables: {year}, {month}, {day}, {hour}, {minute}, {second}, {model}
# Debe ser una ruta relativa y no puede contener ..
template = {year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4

[watch]
# Todos los intervalos deben ser mayores a 0.
poll_interval_secs = 60
poll_interval_idle_secs = 300
idle_threshold_mins = 30
# Rango seguro: 1..16
max_simultaneous = 3

[auth]
# session_cookie = "PHPSESSID=abc123; chaturbatesid=xyz"
```

### Arquitectura

El proyecto sigue una arquitectura Onion/DDD con separación clara de responsabilidades:

```text
src/
├── domain/                  # Lógica de negocio pura
│   ├── value_objects/       # ModelName, StreamUrl, VideoQuality, EstadoModelo
│   ├── repositories/        # Traits
│   └── errors.rs            # Errores de dominio
├── infrastructure/          # Implementaciones concretas
│   ├── external/            # ChaturbateClient
│   └── config/              # AppConfig
├── application/             # Casos de uso
│   ├── cli_controller.rs
│   └── watch_service.rs
└── presentation/            # CLI y salida en consola
    ├── cli/commands.rs
    └── output.rs
```

### Desarrollo

```bash
cargo build
cargo test
cargo build --release
RUST_LOG=debug ./target/debug/cbrec nombremodelo
```

### Troubleshooting

**El modelo no está online**

```text
[ERROR] El modelo 'nombremodelo' no esta online o no se pudo obtener el stream
Puedes verificar el estado con: cbrec check nombremodelo
```

Verifica que el nombre sea correcto y que esté transmitiendo.

**Archivo muy pequeño o movido a `/small`**

El stream duró muy poco o FFmpeg no recibió suficientes datos para una grabación normal. Ajusta `min_file_size` en la configuración si quieres cambiar ese umbral. Para clips cortos intencionales, usa `--duration`.

**Error de red o timeout**

El cliente reintenta automáticamente con backoff exponencial. Si persiste, verifica tu conexión.

**Bloqueo de Cloudflare en modo `watch`**

Configura una `session_cookie` válida para reducir la probabilidad de bloqueo.

**FFmpeg no encontrado**

```bash
cbrec nombremodelo --ffmpeg-path /ruta/a/ffmpeg
CBREC_FFMPEG=/ruta/a/ffmpeg cbrec nombremodelo
```

En Windows, el instalador `.msi` y el paquete `portable.zip` incluyen `ffmpeg.exe`. El binario `cbrec.exe` suelto requiere tener FFmpeg instalado o configurar la ruta.

### Requisitos

- FFmpeg incluido en Windows MSI/portable zip, en el PATH, especificado con `--ffmpeg-path` o con `CBREC_FFMPEG`.
- Conexión a internet.

---

## English

### Description

**cbrec** records Chaturbate streams from the command line. Pass one or more model names and recording starts immediately. With `watch`, cbrec can monitor models in the background and automatically start recording when they go online.

### Features

| Feature | Description |
|---|---|
| Direct recording | No setup required, starts immediately |
| `watch` daemon | Monitors models and records automatically when they go online |
| Authentication | Session cookie support for private/fan-only rooms |
| Adaptive polling | Short interval when activity exists, longer interval when everything is offline |
| Confirmation mode | `--ask` asks before recording each model |
| Parallel recordings | Configurable limit for simultaneous recordings |
| Short clips | `--duration SECS` for timed recordings |
| Flexible FFmpeg | PATH, `--ffmpeg-path`, `CBREC_FFMPEG`, or Windows package with FFmpeg included |
| Clean shutdown | Ctrl+C stops active recordings properly |
| Low resource usage | ~3-5 MB RAM while idle, 0% CPU while sleeping between cycles |
| DDD/Onion | Modular and extensible architecture |

### Installation

#### Prebuilt binaries

Download the binary for your platform from the [releases page](https://github.com/Duna-tools/cbrec/releases):

| Platform | File |
|---|---|
| Linux x86_64 | `cbrec_linux_amd64.tar.gz` |
| Linux x86_64 (Debian/Ubuntu) | `cbrec_*.deb` |
| Linux x86_64 (Fedora/RHEL) | `cbrec_*.rpm` |
| Linux (Arch) | `PKGBUILD` |
| macOS x86_64 | `cbrec_macos_x86_64.tar.gz` |
| macOS Apple Silicon | `cbrec_macos_aarch64.tar.gz` |
| Windows x86_64 | `cbrec_*.msi`, `cbrec_*_portable.zip`, or `cbrec.exe` |

For Arch Linux:

```bash
makepkg -si
```

#### Build from source

Requirements: Rust 1.70+ and FFmpeg available.

```bash
git clone https://github.com/Duna-tools/cbrec.git
cd cbrec
cargo build --release
```

The binary is created at `target/release/cbrec`.

### Usage

#### Direct recording

```bash
# Record one model with automatic quality detection
cbrec modelname

# Record multiple models in parallel
cbrec alice bob charlie

# Use a custom output directory
cbrec modelname -o ~/my_videos

# Choose video quality
cbrec modelname -q 720p

# Record a short 20-second clip
cbrec --duration 20 modelname

# Use a custom FFmpeg path
cbrec modelname --ffmpeg-path /usr/local/bin/ffmpeg

# Use FFmpeg from an environment variable
CBREC_FFMPEG=/usr/local/bin/ffmpeg cbrec modelname
```

The default quality is `best`: cbrec resolves the highest available variant and records that direct URL. When `--duration` is used, the output is treated as an explicit clip and is not marked as a "small file" by the normal long-recording threshold.

You can pass model names or Chaturbate URLs; cbrec normalizes both to the same internal model name before recording, watching, or saving to the list.

#### Monitoring daemon: `watch`

```bash
# Monitor and record automatically
cbrec watch alice bob charlie

# Ask before recording each model
cbrec watch alice bob --ask

# Use custom output directory and quality
cbrec watch alice bob -o ~/recordings -q 1080p

# Use a session cookie
cbrec watch alice bob --session-cookie "PHPSESSID=abc123; chaturbatesid=xyz"
```

Example output:

```text
=== cbrec watch started ===
Monitoring: alice, bob
Press Ctrl+C to stop

[14:30:01][alice] offline
[14:30:01][bob] offline
[14:30:01] Next cycle in 60 s
[14:31:01][alice] ONLINE detected
[14:31:01][alice] Starting recording...
```

With `--ask`:

```text
[14:31:01][alice] ONLINE detected
[14:31:01] Record alice now? [Y/n]: y
[14:31:02][alice] Starting recording...
```

#### Helper commands

```bash
# Check whether a model is online
cbrec check modelname

# Explicit record command
cbrec record alice bob

# Explicitly record a 20-second clip
cbrec record alice --duration 20

# List available qualities
cbrec alice -l

# Check FFmpeg, configuration, output, and watch list
cbrec doctor

# Show help
cbrec --help
cbrec watch --help
```

### Authentication

Use a session cookie to access private or fan-only rooms, or to reduce polling blocks.

Recommended option, configuration file:

```toml
# ~/.config/cbrec/config.toml
[auth]
session_cookie = "PHPSESSID=value; chaturbatesid=value"
```

Option for scripts or temporary sessions:

```bash
CBREC_SESSION_COOKIE="PHPSESSID=value; chaturbatesid=value" cbrec watch alice
```

Compatible but less safe option, command-line flag:

```bash
cbrec watch alice --session-cookie "PHPSESSID=value; chaturbatesid=value"
```

Avoid `--session-cookie` when possible: it may be visible in shell history or process listings.

How to get the cookie:

1. Log in to `chaturbate.com` in your browser.
2. Open DevTools with F12.
3. Go to **Application** > **Cookies** > `https://chaturbate.com`.
4. Copy the values for `PHPSESSID` and `chaturbatesid`.

The cookie works for `watch`, `record`, and `check`.

### Configuration

File: `~/.config/cbrec/config.toml`

```toml
[general]
# Base directory where recordings are stored.
# cb_rec/<model> is created inside this path.
output_root = ~/Videos

# Minimum file size in bytes.
# Smaller files are moved to /small. Must be greater than 0.
min_file_size = 262144000

[naming]
# Variables: {year}, {month}, {day}, {hour}, {minute}, {second}, {model}
# Must be a relative path and cannot contain ..
template = {year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4

[watch]
# All intervals must be greater than 0.
poll_interval_secs = 60
poll_interval_idle_secs = 300
idle_threshold_mins = 30
# Safe range: 1..16
max_simultaneous = 3

[auth]
# session_cookie = "PHPSESSID=abc123; chaturbatesid=xyz"
```

### Architecture

The project follows an Onion/DDD architecture with clear responsibility boundaries:

```text
src/
├── domain/                  # Pure business logic
│   ├── value_objects/       # ModelName, StreamUrl, VideoQuality, EstadoModelo
│   ├── repositories/        # Traits
│   └── errors.rs            # Domain errors
├── infrastructure/          # Concrete implementations
│   ├── external/            # ChaturbateClient
│   └── config/              # AppConfig
├── application/             # Use-case orchestration
│   ├── cli_controller.rs
│   └── watch_service.rs
└── presentation/            # CLI and console output
    ├── cli/commands.rs
    └── output.rs
```

### Development

```bash
cargo build
cargo test
cargo build --release
RUST_LOG=debug ./target/debug/cbrec modelname
```

### Troubleshooting

**The model is not online**

```text
[ERROR] Model 'modelname' is not online or the stream could not be fetched
You can check the state with: cbrec check modelname
```

Check that the model name is correct and that the model is currently broadcasting.

**Small file or moved to `/small`**

The stream was too short or FFmpeg did not receive enough data for a normal recording. Adjust `min_file_size` in the configuration if you want to change that threshold. For intentional short clips, use `--duration`.

**Network error or timeout**

The client retries automatically with exponential backoff. If it persists, check your connection.

**Cloudflare block in `watch` mode**

Configure a valid `session_cookie` to reduce the probability of blocks.

**FFmpeg not found**

```bash
cbrec modelname --ffmpeg-path /path/to/ffmpeg
CBREC_FFMPEG=/path/to/ffmpeg cbrec modelname
```

On Windows, the `.msi` installer and `portable.zip` package include `ffmpeg.exe`. The standalone `cbrec.exe` binary requires FFmpeg to be installed or configured explicitly.

### Requirements

- FFmpeg included in Windows MSI/portable zip, available in PATH, specified with `--ffmpeg-path`, or configured with `CBREC_FFMPEG`.
- Internet connection.

---

## License / Licencia

MIT

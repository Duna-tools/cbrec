# cbrec

Grabador de streams simple, eficiente y de bajo consumo, escrito en Rust.

## Descripción

**cbrec** permite grabar streams de Chaturbate de forma sencilla. Proporciona el nombre del modelo y comienza a grabar inmediatamente. Con el comando `watch` puedes monitorizar varios modelos en segundo plano y que graben automáticamente cuando se conecten.

### Características

| Característica | Descripción |
|---|---|
| Grabación directa | Sin configuración previa, arranca al instante |
| Daemon watch | Monitoriza modelos y graba automáticamente al conectarse |
| Autenticación | Soporte de cookie de sesión para rooms privados/fan-only |
| Polling adaptativo | Intervalo corto con actividad, largo cuando todo está offline |
| Modo confirmación | `--ask` para pedir permiso antes de grabar cada modelo |
| Grabaciones paralelas | Hasta N modelos simultáneos configurable |
| Shutdown limpio | Ctrl+C detiene grabaciones activas correctamente |
| Bajo consumo | ~3–5 MB RAM en reposo, 0% CPU durmiendo entre ciclos |
| DDD/Onion | Arquitectura modular y extensible |

---

## Instalación

### Binarios precompilados (recomendado)

Descarga el binario para tu plataforma desde la [página de releases](https://github.com/Duna-tools/cbrec/releases):

| Plataforma | Archivo |
|---|---|
| Linux x86_64 | `cbrec_linux_amd64.tar.gz` |
| Linux x86_64 (Debian/Ubuntu) | `cbrec_*.deb` |
| Linux x86_64 (Fedora/RHEL) | `cbrec_*.rpm` |
| Linux (Arch) | `PKGBUILD` (ver instrucciones abajo) |
| macOS x86_64 | `cbrec_macos_x86_64.tar.gz` |
| macOS Apple Silicon | `cbrec_macos_aarch64.tar.gz` |
| Windows x86_64 | `cbrec_*.msi`, `cbrec_*_portable.zip` o `cbrec.exe` |

#### Arch Linux (PKGBUILD)
```bash
# Descarga el PKGBUILD del release y ejecuta:
makepkg -si
```

### Compilar desde fuentes

**Requisitos:** Rust 1.70+, ffmpeg en PATH

```bash
git clone https://github.com/Duna-tools/cbrec.git
cd cbrec
cargo build --release
# Binario en: target/release/cbrec
```

---

## Uso

### Grabación directa

```bash
# Grabar un modelo (detección automática de calidad)
cbrec nombremodelo

# Grabar varios modelos en paralelo
cbrec alice bob charlie

# Con directorio de salida personalizado
cbrec nombremodelo -o ~/mis_videos

# Elegir calidad de video
cbrec nombremodelo -q 720p

# Grabar un clip corto de 20 segundos
cbrec --duration 20 nombremodelo

# Con ruta a ffmpeg personalizada
cbrec nombremodelo --ffmpeg-path /usr/local/bin/ffmpeg

# Con variable de entorno
CBREC_FFMPEG=/usr/local/bin/ffmpeg cbrec nombremodelo
```

La calidad por defecto es `best`: cbrec resuelve la variante de mayor resolución disponible y graba esa URL directa. Si usas `--duration`, el archivo se trata como clip explícito y no se marca como "archivo muy pequeño" por el umbral normal de grabaciones largas.

### Daemon de monitorización — `watch`

Monitoriza modelos indefinidamente y graba automáticamente cuando se conectan.

```bash
# Monitorizar y grabar automáticamente
cbrec watch alice bob charlie

# Pedir confirmación antes de grabar cada modelo
cbrec watch alice bob --ask

# Con directorio y calidad personalizados
cbrec watch alice bob -o ~/grabaciones -q 1080p

# Con cookie de sesión (rooms privados/fan-only)
cbrec watch alice bob --session-cookie "PHPSESSID=abc123; chaturbatesid=xyz"
```

Ejemplo de salida en consola (log cronológico):
```
=== cbrec watch iniciado ===
Monitorizando: alice, bob
Presiona Ctrl+C para detener

[14:30:01][alice] offline
[14:30:01][bob] offline
[14:30:01] Próximo ciclo en 60 s
[14:31:01][alice] ONLINE detectado
[14:31:01][alice] Iniciando grabacion...
[14:31:01][bob] offline
[14:31:01] Próximo ciclo en 60 s
[14:32:01][alice] grabando...
[14:32:01][bob] offline
...
^C
[14:45:22] Deteniendo daemon watch...
[14:45:22][alice] Grabacion finalizada → /home/user/Videos/cb_rec/alice/2026.06.02_14.31.01_alice.mp4
```

Con `--ask`:
```
[14:31:01][alice] ONLINE detectado
[14:31:01] ¿Grabar a alice ahora? [Y/n]: y
[14:31:02][alice] Iniciando grabacion...
```

### Comandos auxiliares

```bash
# Verificar si un modelo está online
cbrec check nombremodelo

# Grabar explícitamente (equivalente al uso directo)
cbrec record alice bob

# Grabar explícitamente un clip de 20 segundos
cbrec record alice --duration 20

# Listar calidades disponibles
cbrec alice -l

# Ver ayuda
cbrec --help
cbrec watch --help
```

### Autenticación con cuenta de Chaturbate

Para acceder a rooms privados o de fan-club, o para reducir bloqueos de Cloudflare durante el polling:

**Opción 1 — Flag en línea de comandos:**
```bash
cbrec watch alice --session-cookie "PHPSESSID=valor; chaturbatesid=valor"
```

**Opción 2 — Archivo de configuración (permanente):**
```toml
# ~/.config/cbrec/config.toml
[auth]
session_cookie = "PHPSESSID=valor; chaturbatesid=valor"
```

**Cómo obtener la cookie:**
1. Inicia sesión en chaturbate.com en tu navegador
2. Abre DevTools → F12
3. Ve a **Application** → **Cookies** → `https://chaturbate.com`
4. Copia los valores de `PHPSESSID` y `chaturbatesid`

> **Nota:** La cookie funciona para todos los subcomandos: `watch`, `record` y `check`.

---

## Configuración

Archivo: `~/.config/cbrec/config.toml`

```toml
[general]
# Directorio base donde se guardarán las grabaciones.
# Se crea cb_rec/<modelo> dentro de esta ruta.
output_root = ~/Videos

# Tamaño mínimo de archivo (bytes). Archivos menores se mueven a /small.
# 250 MiB por defecto
min_file_size = 262144000

[naming]
# Plantilla para nombres de archivo.
# Variables: {year}, {month}, {day}, {hour}, {minute}, {second}, {model}
template = {year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4

[watch]
# Intervalo de polling cuando hay actividad reciente (segundos)
poll_interval_secs = 60

# Intervalo de polling cuando todos llevan mucho tiempo offline (segundos)
poll_interval_idle_secs = 300

# Minutos sin actividad para entrar en modo idle
idle_threshold_mins = 30

# Máximo de grabaciones simultáneas en modo watch
max_simultaneous = 3

[auth]
# Cookie de sesión de Chaturbate (opcional)
# session_cookie = "PHPSESSID=abc123; chaturbatesid=xyz"
```

---

## Arquitectura

El proyecto sigue una arquitectura **Onion/DDD** con separación clara de responsabilidades:

```
src/
├── domain/                  # Lógica de negocio pura
│   ├── value_objects/       # ModelName, StreamUrl, VideoQuality, EstadoModelo
│   ├── repositories/        # Traits (StreamRepository)
│   └── errors.rs            # Errores de dominio
├── infrastructure/          # Implementaciones concretas
│   ├── external/            # ChaturbateClient (HTTP + HLS + session_cookie)
│   └── config/              # AppConfig (WatchConfig, AuthConfig)
├── application/             # Orquestación de casos de uso
│   ├── cli_controller.rs    # Despacho de comandos CLI
│   └── watch_service.rs     # Daemon de monitorización
└── presentation/            # Interfaz de usuario
    ├── cli/commands.rs      # Definición de comandos (clap)
    └── output.rs            # Salida en consola
```

---

## Desarrollo

```bash
# Debug build
cargo build

# Tests
cargo test

# Release optimizado
cargo build --release

# Logs detallados
RUST_LOG=debug ./target/debug/cbrec nombremodelo
```

---

## Troubleshooting

**El modelo no está online:**
```
[ERROR] El modelo 'nombremodelo' no esta online o no se pudo obtener el stream
Puedes verificar el estado con: cbrec check nombremodelo
```
Verifica que el nombre sea correcto y que esté transmitiendo.

**Archivo muy pequeño / movido a /small:**
El stream duró muy poco o ffmpeg no recibió suficientes datos para una grabación normal (< 250 MB por defecto). Ajusta `min_file_size` en la config si quieres cambiar ese umbral. Para clips cortos intencionales, usa `--duration`; esos clips se guardan como archivos normales si ffmpeg termina correctamente.

**Error de red / timeout:**
El cliente reintenta automáticamente con backoff exponencial. Si persiste, verifica tu conexión.

**Bloqueo de Cloudflare en modo watch:**
Configura una `session_cookie` válida para reducir la probabilidad de bloqueo (ver sección de autenticación).

**`ffmpeg` no encontrado:**
```bash
# Especifica la ruta manualmente
cbrec nombremodelo --ffmpeg-path /ruta/a/ffmpeg

# O usa la variable de entorno
CBREC_FFMPEG=/ruta/a/ffmpeg cbrec nombremodelo
```

En Windows, el instalador `.msi` y el paquete `portable.zip` incluyen `ffmpeg.exe`.
El binario `cbrec.exe` suelto requiere tener FFmpeg instalado o configurar la ruta.

---

## Requisitos

- **ffmpeg** — incluido en Windows MSI/portable zip, en el PATH del sistema, especificado con `--ffmpeg-path` o con `CBREC_FFMPEG`
- Conexión a internet

---

## Licencia

MIT

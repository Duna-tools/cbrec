# cbrec

Grabador de streams simple y eficiente escrito en Rust.

## Descripcion

cbrec permite grabar streams de Chaturbate de forma sencilla. Solo proporciona el nombre del modelo o la URL y comienza a grabar inmediatamente.

Caracteristicas principales:
- Grabacion directa sin configuracion previa
- Deteccion automatica de finalizacion de stream
- Manejo de Ctrl+C para detencion limpia
- Indicador de progreso en tiempo real
- Arquitectura DDD/Onion modular y extensible

## Instalacion

```bash
# Clonar el repositorio
git clone https://github.com/tu-usuario/cbrec.git
cd cbrec

# Compilar
cargo build --release

# El binario estara en target/release/cbrec
```

## Uso

### Grabacion directa (uso principal)

```bash
# Por nombre de modelo
cbrec nombremodelo

# Con directorio de salida personalizado
cbrec nombremodelo -o ~/mis_videos

# Por URL completa
cbrec https://chaturbate.com/nombremodelo/
```

Ejemplo de salida:
```
=== cbrec - Stream Recorder ===

Modelo: nombremodelo
Verificando disponibilidad...
Modelo online

Archivo: /home/user/Videos/captures/2026.01.26_14.30.00_nombremodelo.mp4

Iniciando grabacion...
Presiona Ctrl+C para detener

---
Descargado: 45.23 MB | Velocidad: 2.15 MB/s

Deteniendo grabacion...

Grabacion finalizada:
  Tamano: 156.78 MB
  Duracion: 00:15:47

Archivo guardado: /home/user/Videos/captures/2026.01.26_14.30.00_nombremodelo.mp4
```

### Comandos auxiliares

```bash
# Verificar si un modelo esta online
cbrec check nombremodelo

# Grabar explicitamente (equivalente al uso directo)
cbrec record nombremodelo
```

## Configuracion

Por defecto las grabaciones se guardan en `~/Videos/captures` con el formato:
```
{year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4
```

Archivo de configuracion: `config/default.toml`

```toml
[general]
output_dir = "~/Videos/captures"
min_file_size = 1024  # bytes, archivos menores se eliminan

[naming]
template = "{year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4"
```

## Arquitectura

El proyecto sigue una arquitectura Onion/DDD con separacion clara de responsabilidades:

```
src/
├── domain/           # Logica de negocio pura
│   ├── value_objects/  # ModelName, StreamUrl
│   ├── repositories/   # Traits (interfaces)
│   └── errors/         # Errores de dominio
├── infrastructure/   # Implementaciones concretas
│   ├── external/       # ChaturbateClient, HLS downloader
│   └── config/         # Configuracion
└── presentation/     # Interfaz de usuario
    └── cli/            # Comandos CLI
```

Ver `doc/ANALISIS_Y_ESPECIFICACION.md` para especificacion completa.

## Desarrollo

```bash
# Compilar debug build
cargo build

# Ejecutar tests
cargo test

# Ejecutar con logs detallados
RUST_LOG=debug ./target/debug/cbrec modelname

# Build optimizado de produccion
cargo build --release
```

## Troubleshooting

**El modelo no se encuentra online:**
```
Error: El modelo 'nombremodelo' no esta online o no se pudo obtener el stream

Puedes verificar el estado con: cbrec check nombremodelo
```
Verifica que el nombre del modelo sea correcto y que este transmitiendo en el sitio.

**Error de permisos al escribir archivo:**
Asegurate de tener permisos de escritura en el directorio de salida configurado.

**Error de red/timeout:**
El cliente reintenta automaticamente 3 veces. Si persiste, verifica tu conexion a internet.

## Requisitos

- Rust 1.70+
- Conexion a internet

## Licencia

MIT

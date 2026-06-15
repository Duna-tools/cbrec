# Lluvia de Ideas: 600 Propuestas para Mejorar cbrec

> [!WARNING]
> **Aviso Importante: Lluvia de Ideas (Brainstorming)**
> Este documento representa una lluvia de ideas abierta y exploratoria para el desarrollo del programa `cbrec`. Dado el carácter creativo y experimental de este brainstorming, se debe tener en cuenta que **algunas ideas pueden ser incorrectas, inviables a nivel técnico, difíciles de implementar, o estar sujetas a limitaciones de las APIs cambiantes de plataformas externas**. El propósito de esta lista es servir de inspiración y hoja de ruta potencial, no una lista de compromisos técnicos garantizados.

Este documento detalla 600 ideas organizadas en 16 categorías clave para mejorar el rendimiento, la experiencia de usuario, la automatización y las capacidades del grabador.

---

## Índice de Categorías
1. [Conectividad y APIs de Chaturbate (Ideas 1-25)](#1-conectividad-y-apis-de-chaturbate)
2. [Grabación y Optimización de Streaming (FFmpeg/HLS) (Ideas 26-50)](#2-grabación-y-optimización-de-streaming-ffmpeghls)
3. [Gestión de Archivos y Almacenamiento (Ideas 51-75)](#3-gestión-de-archivos-y-almacenamiento)
4. [CLI, TUI y Experiencia del Usuario (Ideas 76-100)](#4-cli-tui-y-experiencia-del-usuario)
5. [Arquitectura, DDD y Rendimiento en Rust (Ideas 101-125)](#5-arquitectura-ddd-y-rendimiento-en-rust)
6. [Automatización, Scripts y Notificaciones (Ideas 126-150)](#6-automatización-scripts-y-notificaciones)
7. [Autenticación, Cookies y Manejo de Cloudflare (Ideas 151-175)](#7-autenticación-cookies-y-manejo-de-cloudflare)
8. [Base de Datos, Métricas y Analíticas (Ideas 176-200)](#8-base-de-datos-métricas-y-analíticas)
9. [Autenticación, Cookies y CSRF Avanzado (Ideas 201-250)](#9-autenticación-cookies-y-csrf-avanzado)
10. [APIs REST - Descubrimiento y Búsqueda Detallada (Ideas 251-300)](#10-apis-rest---descubrimiento-y-búsqueda-detallada)
11. [APIs REST - Contexto del Broadcaster y Aplicaciones (Ideas 301-350)](#11-apis-rest---contexto-del-broadcaster-y-aplicaciones)
12. [APIs REST - Chat, Mensajería y PMs (Ideas 351-400)](#12-apis-rest---chat-mensajería-y-pms)
13. [APIs REST - Seguimiento y Fan Club (Ideas 401-450)](#13-apis-rest---seguimiento-y-fan-club)
14. [APIs REST - Propinas y Finanzas (Ideas 451-500)](#14-apis-rest---propinas-y-finanzas)
15. [WebSocket - Autenticación y Conectividad Ably (Ideas 501-550)](#15-websocket---autenticación-y-conectividad-ably)
16. [WebSocket - Eventos de Sala y Lógica de Negocio (Ideas 551-600)](#16-websocket---eventos-de-sala-y-lógica-de-negocio)

---

## 1. Conectividad y APIs de Chaturbate

### 1. Migración a Ably WebSockets para RoomStatusTopic
*   **Por qué:** Evita hacer peticiones HTTP repetitivas (polling) que consumen ancho de banda, CPU y pueden alertar a los sistemas anti-bot de la plataforma.
*   **Cómo:** Conectar un cliente WebSocket a `wss://realtime.pa.highwebmedia.com` tras autenticarse en `/push_service/auth/`, suscribiendo canales de modelos.

### 2. Manejo de Reconexión Automática de WebSockets con Backoff Exponencial
*   **Por qué:** Si la conexión a Ably se pierde por micro-cortes, el daemon debe recuperarse de manera autónoma.
*   **Cómo:** Crear una tarea que detecte desconexiones y reintente el enlace esperando $2^n$ segundos hasta un máximo de 5 minutos.

### 3. Escucha de RoomTipAlertTopic para Prioridad de Grabación
*   **Por qué:** Priorizar ancho de banda o calidad de grabación si el modelo está recibiendo gran volumen de propinas.
*   **Cómo:** Monitorear el evento `RoomTipAlertTopic` y cambiar las configuraciones del stream en caliente si supera límites prefijados.

### 4. Pausa y Reanudación Automática en Shows Privados
*   **Por qué:** Evitar errores 403 o de HLS cuando un modelo inicia show privado y no disponemos de acceso de pago.
*   **Cómo:** Detectar el evento `RoomStatusTopic` para suspender FFmpeg temporalmente y reanudarlo cuando vuelva a público.

### 5. Detección Inteligente de Spy Shows
*   **Por qué:** Optimizar el uso de cookies autorizadas para grabar shows especiales con tarifa reducida.
*   **Cómo:** Leer el campo `current_show` del WebSocket y validar si las cookies de sesión son aptas antes de grabar.

### 6. Rotación Dinámica de Servidores de Fallback de Ably
*   **Por qué:** Resolver bloqueos o caídas locales del balanceador de carga de Ably.
*   **Cómo:** Rotar de forma secuencial por `a-fallback`, `b-fallback`, etc., provistos por la plataforma si falla la dirección base.

### 7. Monitoreo Automatizado de la Lista de Seguidos (/follow/api/online_followed_rooms/)
*   **Por qué:** Elimina la necesidad de listar manualmente a los creadores de interés en ficheros locales.
*   **Cómo:** Realizar consultas programadas a `/follow/api/online_followed_rooms/` con las cookies del usuario para iniciar descargas automáticas.

### 8. Suscripción Dinámica a Canales WebSocket (Hot-Reload)
*   **Por qué:** Modificar la lista de vigilancia de `cbrec watch` sin reiniciar el daemon del programa.
*   **Cómo:** Usar canales asíncronos en Rust para notificar al bucle WebSocket que añada o elimine suscripciones de canales en vivo.

### 9. Extracción Automática del UID del Modelo
*   **Por qué:** Las APIs internas del WebSocket y consultas de historial requieren el UID alfanumérico del modelo.
*   **Cómo:** Realizar un GET a `/api/biocontext/{username}/` para resolver y cachear el UID al vuelo.

### 10. Polling REST como Fallback Resiliente
*   **Por qué:** Continuar monitorizando si se bloquean los WebSockets.
*   **Cómo:** Si el WS falla sistemáticamente, cambiar temporalmente a sondeos en `/api/ts/roomlist/room-list/?search={username}`.

### 11. Clasificación por Hashtags (/api/ts/roomlist/all-tags/)
*   **Por qué:** Organizar las descargas resultantes según la temática del show en tiempo real.
*   **Cómo:** Extraer el array `tags` de la respuesta de la API de la sala y agregarlo a los metadatos JSON del video.

### 12. Monitoreo del Chat para Predicción de Fin de Transmisión
*   **Por qué:** Prevenir cortes abruptos de archivo preparando un cierre de video limpio.
*   **Cómo:** Detectar palabras de despedida en `RoomMessageTopic` para predecir desconexiones inminentes del modelo.

### 13. Verificación Automática de Restricciones de Edad
*   **Por qué:** Evitar bloqueos al intentar descargar streams que exigen confirmación de mayoría de edad.
*   **Cómo:** Validar el campo `is_age_verified` e incluir las cabeceras HTTP de cookies de verificación correspondientes.

### 14. Recomendación de Modelos Similares en Inactividad
*   **Por qué:** Ofrecer alternativas de grabación si los modelos prioritarios están desconectados.
*   **Cómo:** Consultar `/api/more_like/{username}/` y sugerir creadores de contenido parecidos mediante la CLI.

### 15. Detección de Cambios de Backend de Push Service
*   **Por qué:** Chaturbate a veces notifica migraciones de infraestructura que invalidan los sockets.
*   **Cómo:** Capturar el evento `GlobalPushServiceBackendChangeTopic` para refrescar de forma limpia los tokens de conexión.

### 16. Captura de Metas de Propinas (/api/panel_context/)
*   **Por qué:** Almacenar el contexto de objetivos del creador durante el inicio del stream.
*   **Cómo:** Consultar `/api/panel_context/{username}/` al iniciar la descarga y añadirlo a las propiedades del archivo de video.

### 17. Manejo Eficiente de Salas Protegidas con Contraseña
*   **Por qué:** No saturar los logs con errores de conexión infructuosos si la sala se bloquea temporalmente.
*   **Cómo:** Detectar `RoomPasswordProtectedTopic` y posponer el reintento de conexión hasta que se elimine la clave.

### 18. Descarga de Historial de Chat Reciente (/push_service/room_history/)
*   **Por qué:** Disponer del contexto de los minutos previos al inicio de la grabación.
*   **Cómo:** Hacer POST a `/push_service/room_history/` antes de abrir el socket de chat para guardar mensajes anteriores.

### 19. Mapeo de Juegos de la Sala (/api/ts/games/current/)
*   **Por qué:** Catalogar los streams si el creador está jugando a un juego de mesa o interactivo.
*   **Cómo:** Consultar el endpoint de juegos activos al iniciar la grabación y agregarlo como etiqueta de búsqueda.

### 20. Monitoreo de Apps Activas en Sala (/api/public/asp/broadcast/applist/)
*   **Por qué:** Identificar el uso de juguetes interactivos (ej. Lovense) para clasificar el video.
*   **Cómo:** Leer el JSON de `applist` del broadcaster y añadir las aplicaciones activas a los logs.

### 21. Medición de Audiencia en Intervalos (/api/getchatuserlist/)
*   **Por qué:** Disponer de gráficas de popularidad del stream.
*   **Cómo:** Consultar periódicamente `/api/getchatuserlist/` para registrar el número exacto de espectadores en base de datos.

### 22. Base de Datos Local de UIDs (Cache)
*   **Por qué:** Ahorrar peticiones web repetitivas de resolución de identificadores.
*   **Cómo:** Guardar en disco un mapa de persistencia `username -> uid` con tiempo de vida limitado.

### 23. Monitoreo de Notificaciones de Campana Nativas (/notifications/updates/)
*   **Por qué:** Disparar descargas instantáneas basadas en las alertas reales del sitio.
*   **Cómo:** Consumir `/notifications/updates/?notification_type=bell_notification` de la cuenta de usuario.

### 24. Detección Dificultosa y Bypass de Geo-Bloqueo de Salas
*   **Por qué:** Informar adecuadamente si el stream es inaccesible desde la IP actual.
*   **Cómo:** Identificar fallos 403 específicos de HLS relacionados con restricciones geográficas y alertar de la necesidad de proxies.

### 25. Grabación Condicional por Volumen de Espectadores
*   **Por qué:** Ahorrar espacio omitiendo directos con baja audiencia.
*   **Cómo:** Comparar `num_users` de la API con un umbral configurado por el usuario en la CLI antes de grabar.

---

## 2. Grabación y Optimización de Streaming (FFmpeg/HLS)

### 26. Descarga y Parsado Nativo de LL-HLS en Rust (Sin FFmpeg Externo)
*   **Por qué:** Reducir el consumo de CPU de la aplicación eliminando la dependencia forzada de subprocesos externos.
*   **Cómo:** Implementar un lector HTTP asíncrono que analice las listas `.m3u8` y guarde los segmentos en disco de forma nativa.

### 27. Monitoreo de Logs de FFmpeg por Pipes Asíncronos
*   **Por qué:** Detectar caídas o corrupción intermedia en los datos del stream que no cierran el proceso.
*   **Cómo:** Redirigir el `stderr` de FFmpeg a un búfer asíncrono y detectar warnings de sincronización de frames.

### 28. Algoritmo Adaptativo de Reintento HLS
*   **Por qué:** Evitar cortes de grabación causados por retardos temporales de carga en servidores de video.
*   **Cómo:** Reintentar descargas de segmentos fallidos con pausas breves de 200ms antes de descartar la sesión.

### 29. Extracción de Chat a Subtítulos SRT/ASS en Tiempo Real
*   **Por qué:** Visualizar el chat del stream de forma integrada en cualquier reproductor clásico de video.
*   **Cómo:** Grabar los eventos de `RoomMessageTopic` en un fichero de texto con timestamps correspondientes a la duración.

### 30. Segmentación Automática de Archivos de Video
*   **Por qué:** Proteger los datos y facilitar el movimiento de grabaciones de larga duración.
*   **Cómo:** Indicar a FFmpeg que fragmente el archivo de video cada hora o cada determinado número de Gigabytes.

### 31. Reparación Automática de Archivos MP4 Corruptos (Fast-Start)
*   **Por qué:** Garantizar la legibilidad de videos si el grabador se detuvo repentinamente.
*   **Cómo:** Ejecutar `ffmpeg -movflags +faststart` en segundo plano tras cierres abruptos para mover el índice al inicio del video.

### 32. Cambio de Contenedor por Defecto a MKV (.mkv)
*   **Por qué:** Evitar la corrupción de la metadata si la grabación sufre interrupciones imprevistas de energía.
*   **Cómo:** Guardar las grabaciones por defecto en contenedor MKV y remuxar a MP4 únicamente bajo demanda explícita.

### 33. Aceleración por Hardware en Remuxing/Transcodificación
*   **Por qué:** Ahorrar recursos del sistema al comprimir o transcodificar streams en caliente.
*   **Cómo:** Detectar NVENC, QuickSync o VAAPI en el hardware del equipo y aplicar las cabeceras de códec optimizadas en la línea de comando.

### 34. Detección Automática de Pantallas Estáticas o "Away"
*   **Por qué:** Reducir almacenamiento cuando la cámara transmite pantallas de espera.
*   **Cómo:** Medir la fluctuación del bitrate y suspender o ralentizar la toma de frames si la señal de video es estática.

### 35. Grabación Simultánea de Calidad Máxima y Calidad Baja
*   **Por qué:** Contar con previsualizaciones rápidas de poco peso aptas para conexiones móviles.
*   **Cómo:** Descargar en hilos paralelos el playlist HLS de alta resolución (1080p) y el de baja resolución (360p).

### 36. Optimización de Buffers de FFmpeg para Reducir Latencia de Inicio
*   **Por qué:** Evitar perderse los primeros segundos del inicio del show del creador.
*   **Cómo:** Ajustar los parámetros `-analyzeduration` y `-probesize` en valores mínimos de análisis.

### 37. Detección de Silencio Continuo en Audio
*   **Por qué:** Prevenir la grabación inútil de streams mudos por fallos de micrófono del emisor.
*   **Cómo:** Aplicar el filtro de audio `silencedetect` de FFmpeg y alertar si se prolonga durante mucho tiempo.

### 38. Fusión Automatizada de Segmentos Cortados por Micro-cortes
*   **Por qué:** Mantener un único archivo de video unificado en lugar de múltiples fragmentos cortos.
*   **Cómo:** Si hay reconexiones antes de un tiempo límite, concatenar los videos temporales de forma directa sin recodificar.

### 39. Inyección Directa de Metadatos en el Video
*   **Por qué:** Disponer de información de origen integrada dentro del propio archivo multimedia.
*   **Cómo:** Pasar directivas `-metadata` en la inicialización de FFmpeg para guardar nombre, fecha y tags del show.

### 40. Descarte Inteligente de Falsos Positivos de Grabación
*   **Por qué:** Evitar acumular basura en el disco por transmisiones de prueba de pocos segundos.
*   **Cómo:** Eliminar de forma automática cualquier grabación que dura menos de 30 segundos y pese menos de 5MB.

### 41. Limitador de Ancho de Banda (Rate Limiting) por Stream
*   **Por qué:** Compartir la conexión a internet de forma equilibrada con otros servicios del servidor.
*   **Cómo:** Implementar un limitador de flujo de datos (token bucket) en los canales de recepción de sockets de red.

### 42. Modo Solo Audio (Extracción AAC)
*   **Por qué:** Guardar recitales o streams conversacionales consumiendo una décima parte del espacio físico.
*   **Cómo:** Configurar la descarga con `-vn -c:a copy` para conservar solo las pistas de sonido en archivos M4A.

### 43. Generador de Capturas de Pantalla (Thumbnails) Periódicas
*   **Por qué:** Previsualizar el contenido en un gestor de archivos sin abrir el video.
*   **Cómo:** Lanzar tareas periódicas en Rust que guarden fotogramas individuales en JPG cada 5 minutos.

### 44. Creación de Hojas de Contacto de Video (VCS)
*   **Por qué:** Disponer de una vista previa general en formato de collage de todo el directo finalizado.
*   **Cómo:** Generar una imagen cuadriculada con frames de video distribuidos uniformemente al concluir la grabación.

### 45. Monitoreo de Pérdida de Sincronía A/V
*   **Por qué:** Detectar si el desfase entre audio y video arruina la reproducción.
*   **Cómo:** Monitorear logs de FFmpeg y forzar una reconexión rápida si se detectan saltos excesivos en los timestamps.

### 46. Conexión a Stream LL-HLS de Baja Latencia
*   **Por qué:** Reducir el retardo de buffer y capturar el directo lo más cercano posible al tiempo real.
*   **Cómo:** Consultar el manifiesto maestro de reproducción HLS buscando URIs destinadas a baja latencia.

### 47. Modo de Espera Activa para Shows Privados
*   **Por qué:** Reanudar la grabación al instante si un modelo vuelve de un show de pago a público.
*   **Cómo:** Mantener en espera el socket de eventos y disparar el subproceso grabador al recibir la señal de vuelta a público.

### 48. Grabación en Buffer de RAM (tmpfs)
*   **Por qué:** Alargar la vida útil de unidades SSD reduciendo escrituras repetitivas de bytes temporales.
*   **Cómo:** Guardar segmentos de descarga en un volumen RAM y mover el video consolidado al disco físico al concluir.

### 49. Detección de Streams de Retransmisión (Restreams)
*   **Por qué:** Evitar la duplicación de datos y la grabación de directos de bucle repetitivos.
*   **Cómo:** Buscar cadenas de texto indicativas en el título o analizar coincidencias de hashes visuales breves.

### 50. Compresión Tardía en Segundo Plano
*   **Por qué:** Optimizar el almacenamiento de videos antiguos en horas de baja demanda de red y CPU.
*   **Cómo:** Buscar videos guardados antiguos y comprimirlos al formato eficiente AV1 o H.265 usando baja prioridad de proceso.

---

## 3. Gestión de Archivos y Almacenamiento

### 51. Plantilla de Rutas Altamente Configurable
*   **Por qué:** Organizar directorios y subcarpetas según las preferencias de orden del usuario.
*   **Cómo:** Definir plantillas en el TOML (ej. `{model}/{year}/{model}_{date}.mp4`) que se resuelven dinámicamente al grabar.

### 52. Rotación de Almacenamiento con Límite de Espacio
*   **Por qué:** Evitar que el sistema colapse por falta de espacio físico en el disco de descarga.
*   **Cómo:** Monitorear el tamaño total de la carpeta de descargas y borrar videos viejos cuando supere el límite fijado.

### 53. Integración Directa con Rclone / S3
*   **Por qué:** Ahorrar espacio en el servidor local transfiriendo datos a servicios de nube externos.
*   **Cómo:** Subir archivos de video de forma segura mediante APIs S3 o llamadas Rclone de fondo tras finalizar.

### 54. Sumas de Verificación (Checksums) Post-Grabación
*   **Por qué:** Garantizar que los videos no sufren alteraciones ni pérdidas en subidas de red.
*   **Cómo:** Calcular SHA256 al consolidar el archivo y guardarlo en un fichero de verificación `.sha256`.

### 55. Control de Archivos Bloqueados (`.lock`)
*   **Por qué:** Evitar que procesos externos manipulen videos a medio escribir por FFmpeg.
*   **Cómo:** Crear archivos vacíos `.lock` que acompañen a las grabaciones activas y eliminarlos al finalizar la escritura.

### 56. Pausa de Seguridad ante Espacio en Disco Insuficiente
*   **Por qué:** Evitar que un disco lleno corrompa el indexado del MP4 en proceso de guardado.
*   **Cómo:** Chequear periódicamente el espacio libre en el volumen destino; detener descargas si baja de 2GB.

### 57. Limpieza de Archivos Temporales Huérfanos al Arrancar
*   **Por qué:** Eliminar residuos inservibles de sesiones que fallaron por cortes de luz repentinos.
*   **Cómo:** Escanear carpetas temporales al iniciar la aplicación para purgar archivos rotos o incompletos.

### 58. Exportación de Logs de Descarga en Formato JSON por Video
*   **Por qué:** Registrar un historial de la tasa de transferencia, bitrate y errores por cada video de forma independiente.
*   **Cómo:** Guardar detalles técnicos en un archivo JSON homónimo al concluir la grabación del video.

### 59. Evitar Colisiones de Nombres de Archivos mediante Sufijos Inteligentes
*   **Por qué:** Evitar sobreescribir grabaciones previas si se realizan micro-reconexiones simultáneas.
*   **Cómo:** Comprobar existencia previa de rutas y anexar marcadores incrementales tipo `_1`, `_2` o UUIDs al nombre.

### 60. Clasificación Automática Basada en Etiquetas del Modelo
*   **Por qué:** Guardar y agrupar videos similares de acuerdo a los intereses comunes y tags de los modelos.
*   **Cómo:** Leer los hashtags del roomlist y estructurar carpetas organizadoras basadas en las etiquetas dominantes.

### 61. Enlaces Simbólicos para Videos Destacados
*   **Por qué:** Acceder fácilmente a videos largos sin duplicar almacenamiento ni mover ficheros originales.
*   **Cómo:** Crear accesos directos simbólicos en un directorio `highlights/` si la grabación excede un número de horas.

### 62. Medidor del Tiempo de Escritura en Disco para Prevenir Bottlenecks
*   **Por qué:** Evitar que retardos de escritura en discos lentos terminen saturando los buffers de red de entrada.
*   **Cómo:** Cronometrar llamadas de escritura y loguear advertencias si exceden el intervalo crítico de red HLS.

### 63. Empaquetado Automático de Metadatos y Adjuntos
*   **Por qué:** Facilitar el movimiento y archivado de registros secundarios sin desordenar las carpetas de videos.
*   **Cómo:** Empaquetar el JSON de logs, miniaturas JPG y chat SRT en un archivo comprimido junto al video final.

### 64. Escáner de Archivos Huérfanos
*   **Por qué:** Reconstruir registros históricos si los archivos fueron renombrados o movidos manualmente por el usuario.
*   **Cómo:** Añadir el comando `cbrec files scan` para reconstruir ficheros de logs JSON basándose en nombres de archivos.

### 65. Herramienta de Renombrado Masivo Integrada (Bulk Renamer)
*   **Por qué:** Aplicar nuevos esquemas de organización a grabaciones históricas sin alterar su integridad.
*   **Cómo:** Implementar un procesador de plantillas recursivo que actualice rutas antiguas de forma segura.

### 66. Optimización para Sistemas de Archivos Copy-on-Write (Btrfs/ZFS)
*   **Por qué:** Reducir latencias de fragmentación de disco causadas por la descarga concurrente de múltiples streams.
*   **Cómo:** Ejecutar un flag de desactivación de Copy-on-Write en el directorio de descargas si se detecta Btrfs en Linux.

### 67. Creación de Capítulos en MP4/MKV a partir de Cambios de Título del Stream
*   **Por qué:** Facilitar la navegación en el reproductor saltando directamente a los diferentes shows del día.
*   **Cómo:** Capturar cambios en `RoomTitleChangeTopic` y escribir marcas de capítulos nativos en el contenedor del video.

### 68. Carpeta de Tránsito o Staging Dedicada
*   **Por qué:** Optimizar la velocidad de descarga grabando en SSD rápido y moviendo a HDD masivo al finalizar.
*   **Cómo:** Definir parámetros de rutas temporales y de almacenamiento definitivo; mover los archivos de fondo asíncronamente.

### 69. Configuración Fragmentada en Directorio `conf.d/`
*   **Por qué:** Organizar listas de modelos y credenciales de forma modular.
*   **Cómo:** Analizar y mezclar en caliente los ficheros TOML independientes alojados en una carpeta de configuración.

### 70. Exportación de Registro de Grabaciones a Base de Datos Centralizada
*   **Por qué:** Agilizar consultas complejas de búsqueda sobre el total de descargas realizadas.
*   **Cómo:** Escribir registros históricos en una base SQLite local de manera uniforme tras finalizar cada tarea de grabación.

### 71. Consolidación de Micro-videos (Defragmenter)
*   **Por qué:** Unificar grabaciones fragmentadas por caídas continuas de red en un único archivo limpio.
*   **Cómo:** Integrar el comando `cbrec files consolidate` que concatena flujos mediante la herramienta nativa de FFmpeg.

### 72. Monitoreo de Desgaste TBW (Total Bytes Written)
*   **Por qué:** Proteger el hardware del usuario avisando si el volumen de escritura acelera el desgaste físico del disco.
*   **Cómo:** Consultar contadores SMART del disco destino e informar del estado de salud física de la unidad.

### 73. Detección de Archivos Inusualmente Pequeños (Zero-Byte Files)
*   **Por qué:** Limpiar logs y directorios de archivos temporales fallidos que quedaron vacíos.
*   **Cómo:** Eliminar de forma automática cualquier fichero menor a 1KB detectado tras finalizar tareas de grabación.

### 74. Cifrado Opcional de Archivos Grabados en Disco
*   **Por qué:** Garantizar la privacidad y seguridad del contenido multimedia guardado en el servidor local.
*   **Cómo:** Implementar encriptación AES al vuelo durante el streaming utilizando una llave pública del usuario.

### 75. Notificación Automática a Plex / Jellyfin
*   **Por qué:** Actualizar bibliotecas del centro multimedia local sin forzar escaneos manuales.
*   **Cómo:** Realizar una petición POST HTTP con tokens de API configurados a los servidores Jellyfin o Plex al terminar.

---

## 4. CLI, TUI y Experiencia del Usuario

### 76. Interfaz de Terminal Interactiva (TUI) con Ratatui
*   **Por qué:** Controlar de forma amigable todas las grabaciones concurrentes en un único panel de consola.
*   **Cómo:** Desarrollar vistas dinámicas con `ratatui` que expongan bitrates, barras de progreso y logs coloreados.

### 77. Panel TUI para Control Dinámico de Modelos
*   **Por qué:** Gestionar la lista de vigilancia al vuelo sin reiniciar procesos del daemon en segundo plano.
*   **Cómo:** Exponer un input interactivo que permita agregar o quitar modelos de la monitorización presionando teclas directas.

### 78. Generación Automatizada de Completados de Shell (Autocomplete)
*   **Por qué:** Agilizar la escritura de comandos del grabador en la terminal de Linux.
*   **Cómo:** Generar definiciones de autocompletado para Zsh, Bash y Fish aprovechando las características de Clap.

### 79. Visualización de Miniaturas Sixel/Kitty en Terminal
*   **Por qué:** Visualizar visualmente si la transmisión es de interés sin abrir herramientas gráficas de escritorio.
*   **Cómo:** Renderizar previsualizaciones usando protocolos gráficos modernos de terminal sobre la TUI activa.

### 80. Prompt Interactivo del Modo Confirmación `--ask` Mejorado
*   **Por qué:** Prevenir que prompts interactivos bloqueen el daemon watch de forma indefinida en de la ausencia del usuario.
*   **Cómo:** Agregar timeouts que seleccionen una acción predeterminada tras varios segundos de inactividad del usuario.

### 81. Comando de Chequeo Rápido (`cbrec check`) con Formato de Tabla Colorida
*   **Por qué:** Interpretar estados de múltiples modelos de un vistazo de forma estructurada.
*   **Cómo:** Imprimir tablas estructuradas usando el crate `tabled` con columnas de estado, calidad y espectadores.

### 82. Soporte para Temas de Colores Personalizados en la Consola
*   **Por qué:** Ajustar la paleta de colores de logs y TUI según las preferencias visuales del usuario.
*   **Cómo:** Cargar perfiles de colores ANSI definidos en la configuración de usuario TOML.

### 83. Flag Global `--json` para Integración Externa
*   **Por qué:** Facilitar el parseo de la salida del grabador desde scripts de Bash usando herramientas como `jq`.
*   **Cómo:** Estructurar y serializar la respuesta de comandos principales a formato JSON si se detecta la flag.

### 84. Barra de Progreso Compacta por Modelo (Multi-Progress Bars)
*   **Por qué:** Evitar logs ininterrumpidos y confusos cuando se graban decenas de creadores a la vez.
*   **Cómo:** Integrar `indicatif` para pintar y actualizar barras de descarga en vivo de forma de persistente en la terminal.

### 85. Comando CLI de Edición de Configuración Asistida (`cbrec config edit`)
*   **Por qué:** Evitar fallos de sintaxis TOML verificando la validez del archivo de configuración antes de guardarlo.
*   **Cómo:** Abrir la configuración con el editor por defecto y analizar errores estructurales antes de confirmar cambios.

### 86. Diagnóstico del Sistema Integrado (`cbrec doctor`)
*   **Por qué:** Detectar problemas comunes de red, dependencias o permisos de forma automática.
*   **Cómo:** Implementar tests internos que evalúen la conexión HTTP, versiones de FFmpeg y permisos de carpetas.

### 87. Logs Aislados y Prefijados por Modelo
*   **Por qué:** Leer el progreso de un modelo específico de forma aislada sin mezclar líneas de log.
*   **Cómo:** Utilizar prefijos dinámicos con colores asignados aleatoriamente a cada hilo de grabación en la CLI.

### 88. Lista Rápida de Videos Guardados (`cbrec list-videos`)
*   **Por qué:** Conocer rápidamente el estado e inventario local de grabaciones completadas.
*   **Cómo:** Escanear directorios configurados y retornar tablas estructuradas con metadatos de archivos de video.

### 89. Buscador CLI Interactivo
*   **Por qué:** Localizar un video antiguo entre miles de archivos usando filtros dinámicos.
*   **Cómo:** Implementar búsqueda difusa interactiva de grabaciones directamente en la terminal.

### 90. Modo Silencioso Inteligente (`--quiet` / `--silent`)
*   **Por qué:** Reducir escrituras y polución de logs en configuraciones integradas en Docker u otros daemons.
*   **Cómo:** Desactivar salidas de stdout normales, enviando solo errores críticos a los flujos principales.

### 91. Atajos de Teclado Globales en el Daemon Watch
*   **Por qué:** Interactuar rápidamente con las tareas activas sin ingresar subcomandos complejos.
*   **Cómo:** Capturar inputs en crudo de teclado para pausar o cancelar descargas activas directamente.

### 92. Comando de Visualización de Recursos (`cbrec top`)
*   **Por qué:** Monitorear el consumo de CPU y RAM de cada instancia de FFmpeg y hilos de Rust en vivo.
*   **Cómo:** Mostrar estadísticas de rendimiento y red detalladas de cada subproceso activo.

### 93. Envase de Errores Amigable para Humanos (User-Friendly Errors)
*   **Por qué:** Ayudar a resolver incidencias sin abrumar con stack traces del compilador.
*   **Cómo:** Mapear pánicos y fallas críticas de red a mensajes descriptivos y soluciones propuestas claras.

### 94. Tail Dinámico de Logs por Modelo
*   **Por qué:** Seguir los eventos técnicos de un creador en tiempo real sin distraerse con otras tareas.
*   **Cómo:** Comando `cbrec logs <modelo>` que lee un buffer en memoria reservado para las trazas de ese hilo.

### 95. Comando Dry Run para Simular Grabaciones
*   **Por qué:** Asegurar que la configuración y permisos son correctos antes de consumir almacenamiento.
*   **Cómo:** Realizar peticiones API y mocks de FFmpeg simulando inicios de descarga sin guardar bytes reales.

### 96. Ayuda Contextual Detallada con Ejemplos Prácticos
*   **Por qué:** Acelerar el aprendizaje del uso de subcomandos avanzados de la CLI.
*   **Cómo:** Extender los bloques de ayuda del comando Clap agregando casos reales de configuración y uso diario.

### 97. Notificaciones de Escritorio Locales
*   **Por qué:** Recibir avisos visuales en el ordenador cuando un modelo prioritario inicie show.
*   **Cómo:** Enviar mensajes push al escritorio del sistema operativo usando el crate `notify-rust`.

### 98. Comando de Auto-Actualización de Binarios (`cbrec update`)
*   **Por qué:** Mantener el grabador actualizado con parches de APIs sin intervención técnica manual.
*   **Cómo:** Consultar las releases de GitHub, descargar la versión correspondiente y reemplazar el binario de forma segura.

### 99. Indicador de Estado de la Sesión en Consola
*   **Por qué:** Verificar que las cookies provistas no han caducado antes de que inicien transmisiones clave.
*   **Cómo:** Mostrar en la CLI de forma clara si el estado de autenticación actual es válido o anónimo.

### 100. Resumen Mensual de Grabaciones (`cbrec stats`)
*   **Por qué:** Conocer métricas consolidadas del grabador (tiempos totales de directo, espacio ahorrado).
*   **Cómo:** Consultar la base SQLite interna para retornar resúmenes visuales agrupados de las grabaciones.

---

## 5. Arquitectura, DDD y Rendimiento en Rust

### 101. Configuración Fina del Scheduler Asíncrono de Tokio
*   **Por qué:** Evitar retrasos en el procesamiento del canal WebSocket causados por la escritura en disco de streams pesados.
*   **Cómo:** Asignar hilos de trabajo diferenciados en Tokio para la gestión de sockets rápidos y la persistencia de video.

### 102. Desacoplamiento mediante Canales Asíncronos (`tokio::sync::mpsc`)
*   **Por qué:** Proteger la descarga de paquetes de red de caídas causadas por latencias temporales del disco duro.
*   **Cómo:** Transferir fragmentos recibidos a colas en RAM, procesando la escritura en disco de forma paralela.

### 103. Patrón Repository Estricto en Domain
*   **Por qué:** Facilitar el testing unitario y la intercambiabilidad de los motores de almacenamiento.
*   **Cómo:** Declarar interfaces de repositorio para lecturas de configuraciones e históricos del dominio.

### 104. Abstracción del Ejecutable FFmpeg para Pruebas de Integración
*   **Por qué:** Testear la reacción del grabador ante caídas de procesos de video sin invocar descargas reales.
*   **Cómo:** Inyectar mocks del subproceso grabador que simulen outputs de error en los tests de Rust.

### 105. Arquitectura Basada en Actores Asíncronos
*   **Por qué:** Aislar los estados de grabación de cada creador en hilos y entornos lógicos independientes.
*   **Cómo:** Modelar cada canal en watch como un Actor asíncrono que recibe y procesa mensajes específicos.

### 106. Serialización Zero-Allocation con Préstamos de Serde
*   **Por qué:** Reducir al mínimo las asignaciones de memoria al parsear grandes flujos de datos JSON.
*   **Cómo:** Deserializar payloads JSON del chat usando referencias temporales a los buffers de red (`&str`).

### 107. Máquina de Estados para Modelos en Watch
*   **Por qué:** Simplificar la lógica de transiciones de estado de conexión y grabación en el dominio.
*   **Cómo:** Implementar un enum de estados rígido y forzar transiciones seguras mediante firmas del compilador.

### 108. Uso de Crate `parking_lot` para Primitivas de Sincronización
*   **Por qué:** Reducir latencias causadas por bloqueos de Mutex en hilos concurrentes del daemon.
*   **Cómo:** Reemplazar locks de la biblioteca estándar por Mutex optimizados del crate `parking_lot`.

### 109. Captura Segura de Pánicos en Grabadores Secundarios (`catch_unwind`)
*   **Por qué:** Evitar que fallos imprevistos en un hilo de descarga colapsen todo el daemon de monitoreo.
*   **Cómo:** Capturar pánicos y recuperar el estado de hilos secundarios sin detener el proceso principal.

### 110. Optimización del Binario en Cargo (LTO & Stripping)
*   **Por qué:** Generar ejecutables ultraligeros ideales para servidores de bajos recursos.
*   **Cómo:** Habilitar LTO y optimizaciones de compilación en el perfil `release` de `Cargo.toml`.

### 111. Cliente HTTP Abstraído para Redundancia de Librería
*   **Por qué:** Cambiar dependencias del cliente de red sin alterar las llamadas de dominio de la API de Chaturbate.
*   **Cómo:** Envolver el cliente de red en un adaptador abstracto inyectable a la infraestructura.

### 112. Integración de Logs Estructurados con Tracing
*   **Por qué:** Seguir el flujo de eventos asíncronos correlacionando trazas técnicas con nombres de modelos.
*   **Cómo:** Utilizar `tracing` para habilitar contextos y spans coloreados en los hilos asíncronos del daemon.

### 113. Perfilado de Memoria Integrado con Jemalloc
*   **Por qué:** Diagnosticar fugas de memoria al ejecutar el programa de forma ininterrumpida por meses.
*   **Cómo:** Usar Jemalloc como asignador y configurar volcados para detectar consumos persistentes.

### 114. Bus de Eventos de Dominio Asíncrono
*   **Por qué:** Desacoplar módulos independientes (webhooks, base de datos) del bucle principal de monitorización.
*   **Cómo:** Implementar un broadcast channel que reciba y propague eventos del dominio en vivo.

### 115. Uso de `Cow<'a, str>` en Estructuras de Datos
*   **Por qué:** Evitar copias innecesarias de identificadores repetitivos de modelos y URLs en memoria.
*   **Cómo:** Tipar campos de texto de forma persistente con `Cow` para manejar referencias estáticas y dinámicas según convenga.

### 116. Mecanismo de Backpressure para Procesamiento de Logs
*   **Por qué:** Evitar sobrecargas de memoria RAM si la velocidad de logs del chat supera la escritura de disco.
*   **Cómo:** Utilizar canales acotados en memoria para suspender la ingesta de eventos si los buffers se llenan.

### 117. División del Proyecto en Workspace de Cargo (Monorepo)
*   **Por qué:** Modularizar el proyecto según Onion architecture y acelerar los tiempos de compilación.
*   **Cómo:** Separar la lógica de negocio, adaptadores externos y TUI en sub-crates independientes.

### 118. Cero Copias de Datos con `bytes::Bytes`
*   **Por qué:** Agilizar el volcado de buffers de video a archivos sin reasignar memoria RAM.
*   **Cómo:** Transportar fragmentos descargados utilizando la estructura optimizada `Bytes` de Rust.

### 119. Tipado Fuerte para Nombres de Modelos
*   **Por qué:** Garantizar que los nombres procesados por red y carpetas cumplen con la sintaxis del sitio.
*   **Cómo:** Envolver strings de nombres de modelos en un tipo struct `ModelName` que valide en tiempo de construcción.

### 120. Jerarquía Detallada de Errores con `thiserror`
*   **Por qué:** Facilitar el diagnóstico de fallas mapeando errores exactos en los subprocesos.
*   **Cómo:** Definir variantes claras para errores HTTP, de deserialización, de disco y de comandos del sistema.

### 121. Colecciones Libres de Bloqueos (Lock-Free Collections)
*   **Por qué:** Evitar contenciones de lectura y escritura concurrentes en mapas globales de modelos en watch.
*   **Cómo:** Usar mapas de persistencia concurrentes como `dashmap` en lugar de Mutex estándar de Rust.

### 122. Recolección de Basura Dinámica en Memoria
*   **Por qué:** Liberar recursos del sistema si la lista de modelos inactivos monitorizados crece en exceso.
*   **Cómo:** Purgar referencias a modelos que llevan demasiado tiempo inactivos en las estructuras de RAM.

### 123. Cargo Features para Compilación Condicional
*   **Por qué:** Permitir compilar binarios mínimos exentos de la interfaz TUI para servidores integrados.
*   **Cómo:** Estructurar dependencias opcionales en el archivo Cargo.toml utilizando features configurables.

### 124. Conexiones HTTP Directas mediante Hyper
*   **Por qué:** Maximizar la tasa de transferencia de datos eliminando overhead de librerías HTTP secundarias.
*   **Cómo:** Conectar sockets usando de forma directa `hyper` para el procesamiento crítico de video.

### 125. Pruebas Basadas en Propiedades con Proptest
*   **Por qué:** Verificar que el analizador de HLS es inmune a corrupción de archivos en red.
*   **Cómo:** Generar variaciones aleatorias del formato `.m3u8` y pasarlas al validador para confirmar su robustez.

---

## 6. Automatización, Scripts y Notificaciones

### 126. Webhooks Nativos de Eventos
*   **Por qué:** Permitir integraciones flexibles con servidores personales del usuario.
*   **Cómo:** Enviar peticiones POST JSON a URLs configuradas en el ciclo del grabador al detectar estados.

### 127. Notificaciones Directas a Telegram
*   **Por qué:** Recibir avisos y alertas del servidor al móvil en tiempo real de forma directa.
*   **Cómo:** Conectar con la API de bots de Telegram usando configuraciones de tokens en el TOML.

### 128. Soporte para Webhooks Estéticos en Discord
*   **Por qué:** Publicar notificaciones elegantes con embeds coloreados en canales de Discord.
*   **Cómo:** Formatear datos del stream a la sintaxis del webhook de Discord al arrancar o detener.

### 129. Scripts de Post-Procesamiento Definidos por el Usuario
*   **Por qué:** Automatizar el procesado de videos finales (ej. mover a NAS) mediante scripts externos.
*   **Cómo:** Configurar un comando shell a invocar y pasarle parámetros como ruta y modelo tras grabar.

### 130. Notificaciones D-Bus Nativas en Linux
*   **Por qué:** Enviar notificaciones de escritorio en Linux integradas con el entorno gráfico sin dependencias complejas.
*   **Cómo:** Conectar con el daemon D-Bus para emitir alertas visuales al iniciar descargas de modelos.

### 131. Integración con Servicios Push (Gotify / Pushover / NTFY)
*   **Por qué:** Enviar notificaciones push nativas a dispositivos sin dependencias de chat complejas.
*   **Cómo:** Soportar llamadas estructuradas a los endpoints de servicios como Gotify o ntfy.

### 132. Script de Instalación Automatizada para Linux
*   **Por qué:** Agilizar la puesta en marcha inicial del grabador en servidores limpios.
*   **Cómo:** Proveer un script autodescargable en Bash que instale FFmpeg y configure el daemon.

### 133. Plantilla y Servicio Systemd con Reinicio Automático
*   **Por qué:** Mantener el grabador corriendo de forma desasistida tras caídas o reinicios del servidor.
*   **Cómo:** Proveer una unidad `cbrec.service` configurada con políticas de reinicio automático robustas.

### 134. Soporte MQTT para Integración con Home Assistant
*   **Por qué:** Disparar acciones domóticas en casa inteligente basadas en los estados del grabador.
*   **Cómo:** Publicar temas MQTT del estado de los modelos hacia brokers centralizados.

### 135. Resúmenes Diarios por Correo Electrónico (SMTP)
*   **Por qué:** Recibir estadísticas de descargas y eventos del día sin abrir consolas.
*   **Cómo:** Implementar envío de emails estructurados a través de conexiones SMTP de usuario al finalizar el día.

### 136. Servidor HTTP REST de Control Embebido
*   **Por qué:** Controlar de forma remota el estado del grabador mediante paneles web del usuario.
*   **Cómo:** Exponer endpoints REST en el daemon para permitir adiciones o pausas de descargas al vuelo.

### 137. Auto-Arranque tras Suspensión de Red
*   **Por qué:** Evitar bloqueos permanentes tras periodos de desconexión o suspensión del sistema operativo.
*   **Cómo:** Monitorizar cambios de red y reiniciar conexiones caídas asíncronamente al detectar conectividad.

### 138. Obtención de Cookies desde Gestores de Contraseñas (Vaults)
*   **Por qué:** Incrementar la seguridad de las claves y cookies de sesión en el servidor.
*   **Cómo:** Integrar llamadas para invocar utilidades como `bw` (Bitwarden) para leer cookies seguras.

### 139. Notificaciones Nativas en Matrix Chat
*   **Por qué:** Publicar alertas en redes descentralizadas de Matrix sin intermediarios.
*   **Cómo:** Formatear llamadas a salas de chat utilizando credenciales de usuario Matrix configuradas.

### 140. API de Grabación Externa (Coordinación)
*   **Por qué:** Evitar descargas duplicadas si el usuario ya está visualizando el directo en otro dispositivo.
*   **Cómo:** Consultar un endpoint del usuario para saber si debe suspenderse la grabación activa en curso.

### 141. Script de Mantenimiento y Purga Automática (Cron)
*   **Por qué:** Mantener el volumen del disco controlado de forma automática en base a reglas de duración.
*   **Cómo:** Añadir subcomandos de purga ejecutable de fondo para borrar fragmentos excesivamente cortos.

### 142. Creación Automática de Entradas Markdown para Sitios Estáticos
*   **Por qué:** Indexar y catalogar videos en webs estáticas personales (Hugo, Jekyll) tras grabar.
*   **Cómo:** Escribir archivos `.md` formateados con metadatos del video en carpetas seleccionadas.

### 143. Control del Daemon mediante Comandos del Chat de Telegram
*   **Por qué:** Modificar configuraciones del grabador de forma remota desde el móvil sin acceso SSH.
*   **Cómo:** Hacer que el bot analice mensajes entrantes de administradores y responda con acciones del sistema.

### 144. Perfiles de Rendimiento Basados en Horarios (Cron-Schedules)
*   **Por qué:** Equilibrar el ancho de banda descargando en menor calidad en horas de alto uso de red.
*   **Cómo:** Permitir perfiles horarios TOML para cambiar las calidades deseadas de los modelos monitoreados.

### 145. Integración con IFTTT (If This Then That)
*   **Por qué:** Integrar el grabador con múltiples servicios web sin programar APIs propietarias.
*   **Cómo:** Emitir peticiones webhooks HTTP formateadas a servicios externos al arrancar grabaciones.

### 146. Alerta de Cookies Prontas a Expirar
*   **Por qué:** Prevenir fallos en salas de pago renovando la sesión de cookies antes de que caduquen.
*   **Cómo:** Evaluar los tiempos de caducidad en el JSON de cookies y alertar 3 días antes de expirar.

### 147. Ejecución de Scripts Pre-Grabación (Validación)
*   **Por qué:** Confirmar condiciones de red (ej. VPN activa) antes de lanzar el subproceso grabador.
*   **Cómo:** Invocar scripts locales y abortar el arranque de FFmpeg si la llamada retorna errores.

### 148. Centralización de Logs de Error con Promtail / Loki
*   **Por qué:** Diagnosticar fallas de múltiples instancias de grabación desde un dashboard unificado.
*   **Cómo:** Estructurar las salidas de log para facilitar su recolección por colectores de Grafana Loki.

### 149. Pausa Cruzada de Grabaciones con Otras Plataformas
*   **Por qué:** Priorizar grabaciones de mejor resolución en otras redes si transmiten en simultáneo.
*   **Cómo:** Consultar APIs externas para suspender descargas en Chaturbate si emiten en plataformas preferenciales.

### 150. Apagado Automático del Servidor tras Cola Vacía
*   **Por qué:** Ahorrar energía apagando servidores domésticos tras concluir shows programados de noche.
*   **Cómo:** Flag especial que lance comandos de apagado seguro (`shutdown`) si no hay directos activos.

---

## 7. Autenticación, Cookies y Manejo de Cloudflare

### 151. Integración con FlareSolverr para Solución de JS Challenges
*   **Por qué:** Cloudflare puede bloquear peticiones si no se resuelven los retos de JavaScript de inicio.
*   **Cómo:** Enrutar opcionalmente llamadas de inicio de sesión a través de FlareSolverr para recibir cookies válidas.

### 152. Rotación Coherente de User-Agents
*   **Por qué:** Evitar firmas de red repetitivas que delaten el uso de scripts de automatización.
*   **Cómo:** Rotar de forma inteligente entre User-Agents modernos en inicios de sesión del daemon.

### 153. Extracción de Cookies desde Navegadores del Sistema
*   **Por qué:** Facilitar la obtención de cookies sin obligar a copiar cadenas de texto complejas de consola.
*   **Cómo:** Leer de forma automática cookies de los perfiles de Chrome o Firefox del usuario mediante crates Rust.

### 154. Rotación Dinámica de Proxies HTTP/SOCKS5
*   **Por qué:** Recuperar descargas de forma automática si la IP principal es baneada o limitada temporalmente.
*   **Cómo:** Cambiar de proxy en vivo ante la presencia de códigos 403 o rate limit en red.

### 155. Notificación Remota de Desafíos Captcha
*   **Por qué:** Permitir al usuario desbloquear logins protegidos por captchas manuales de forma remota.
*   **Cómo:** Guardar el reto temporalmente y alertar al móvil del usuario para que ingrese la solución desde su navegador.

### 156. Extracción y Renovación Automatizada de CSRF Tokens
*   **Por qué:** Las llamadas REST de tipo POST exigen tokens CSRF para evitar errores de protección del sitio.
*   **Cómo:** Mapear y actualizar tokens de cookies en las cabeceras `X-CSRFToken` de peticiones posteriores de forma de transparente.

### 157. Guardado Seguro de Credenciales Cifradas
*   **Por qué:** Facilitar reinicios de sesión autónomos sin comprometer la seguridad de contraseñas.
*   **Cómo:** Utilizar Keyrings del sistema para almacenar y leer de forma cifrada credenciales de usuario.

### 158. Detección Inteligente de Shadowban de IP
*   **Por qué:** Descubrir si la plataforma bloquea silenciosamente peticiones devolviendo listados de salas vacíos.
*   **Cómo:** Alertar del bloqueo si las llamadas de roomlist retornan listas vacías de forma de persistente.

### 159. Algoritmo de Simulación de Comportamiento Humano (Jitter)
*   **Por qué:** Ocultar el uso de scripts evitando llamadas REST en intervalos matemáticos precisos.
*   **Cómo:** Añadir desviaciones aleatorias (jitter) al tiempo de refresco en los bucles del daemon.

### 160. Extensión Ligera de Navegador para Sincronizar Cookies
*   **Por qué:** Automatizar la actualización de cookies sin acceder a terminales ni archivos de configuración.
*   **Cómo:** Proveer una extensión que capte y envíe cookies al servidor REST local de `cbrec` en caliente.

### 161. Soporte para Archivos de Cookies Formato Netscape
*   **Por qué:** Permitir la carga de cookies exportadas por utilidades comunes de terceros.
*   **Cómo:** Crear un parser nativo para leer ficheros `cookies.txt` tradicionales de forma de directa.

### 162. Chequeo de Estado de Autenticación al Arrancar
*   **Por qué:** Prevenir fallos catastróficos en grabaciones alertando temprano si la sesión no es válida.
*   **Cómo:** Intentar cargar endpoints de perfil en la inicialización y advertir al usuario si son rechazados.

### 163. Aislamiento de Cookies de Sesión por Modelo
*   **Por qué:** Usar perfiles o cuentas independientes para monitorear distintos grupos de creadores.
*   **Cómo:** Asignar contenedores de cookies (`CookieJar`) independientes en las tareas de red de cada modelo en watch.

### 164. Mitigación de Rate Limiting con Caché de Respuestas
*   **Por qué:** Proteger la IP del servidor de límites de cuota por llamadas de listado recurrentes.
*   **Cómo:** Guardar resultados de roomlist en RAM por 30 segundos si la frecuencia de peticiones sube temporalmente.

### 165. Configuración de Subdominio de Idioma Fijo
*   **Por qué:** Evitar fallos de URLs causados por redireccionamientos automáticos de idioma del servidor.
*   **Cómo:** Cargar cabeceras `language=en` y `en_subdomain=1` de forma obligatoria en la inicialización de peticiones.

### 166. Renovación en Caliente de Cookies de Sesión
*   **Por qué:** Mantener el estado de login en la nube enviando trazas de actividad periódicas.
*   **Cómo:** Enviar peticiones ligeras de perfil a intervalos para que el servidor extienda la validez de cookies.

### 167. Encriptación Local del Archivo de Cookies
*   **Por qué:** Proteger datos de sesión contra lecturas maliciosas en el almacenamiento local.
*   **Cómo:** Encriptar el fichero de cookies del grabador en disco usando cifrado simétrico AES con contraseña.

### 168. Pool de Cuentas para Balanceo de Carga
*   **Por qué:** Dividir las peticiones entre varias cuentas para no alertar de comportamientos inusuales.
*   **Cómo:** Rotar de forma balanceada la cuenta de red activa al realizar consultas masivas de salas.

### 169. Uso Exclusivo de Cookies Fan-Only
*   **Por qué:** Minimizar el uso de la sesión principal limitando privilegios en grabaciones públicas.
*   **Cómo:** Usar credenciales estándar para búsquedas y cookies premium solo en salas de pago del fan club.

### 170. Pausa Proactiva ante Bloqueos en Cascada
*   **Por qué:** Evitar baneo permanente de IPs pausando peticiones ante señales de bloqueo de forma severa.
*   **Cómo:** Hibernar el daemon watch por 30 minutos si se reciben códigos 403 seguidos de Cloudflare.

### 171. Imitación de Firma TLS (JA3 Fingerprint)
*   **Por qué:** Engañar a firewalls avanzados que analizan las características criptográficas del handshake de red.
*   **Cómo:** Configurar la librería de red Rust para simular la secuencia TLS de Google Chrome de forma de estricta.

### 172. Tráfico de Simulación Lateral
*   **Por qué:** Hacer que el grabador parezca un navegador real consumiendo estáticos además de APIs.
*   **Cómo:** Solicitar periódicamente assets menores (imágenes, CSS) de forma aleatoria de fondo.

### 173. Cabeceras Dinámicas Client Hints (Sec-Ch-Ua)
*   **Por qué:** Evitar inconsistencias de firma en cabeceras de red modernas evaluadas por Cloudflare.
*   **Cómo:** Configurar cabeceras de Client Hints dinámicamente emparejadas al User-Agent provisto.

### 174. Soporte para Cabeceras e Infraestructura Web Móvil
*   **Por qué:** Beneficiarse de filtros de seguridad reducidos orientados a clientes de móviles.
*   **Cómo:** Simular cabeceras HTTP de Safari en iOS o Chrome en Android en los flujos de red.

### 175. Detección Temprana de Invalidación de Sesión por Cambio de Contraseña
*   **Por qué:** Interrumpir ciclos de error si se invalidaron credenciales desde el panel web principal.
*   **Cómo:** Validar redireccionamientos a páginas de reinicio de claves y apagar el daemon de forma de controlada.

---

## 8. Base de Datos, Métricas y Analíticas

### 176. Integración de Base de Datos SQLite Local
*   **Por qué:** Disponer de registros estructurados rápidos de grabaciones, eliminando archivos de texto planos.
*   **Cómo:** Guardar e indexar datos de descargas en SQLite (`rusqlite`) local de forma estructurada.

### 177. Exportador de Métricas de Prometheus
*   **Por qué:** Integrar telemetría de rendimiento técnico del grabador en paneles de Grafana del usuario.
*   **Cómo:** Levantar un endpoint local `/metrics` compatible con servidores Prometheus para recolectar datos.

### 178. Registro Histórico de Propinas Recibidas
*   **Por qué:** Correlacionar duraciones y momentos álgidos del stream con volumen de donaciones.
*   **Cómo:** Escribir en base de datos la cantidad de tokens acumulada durante la descarga de cada sesión.

### 179. Soporte para Series Temporales en InfluxDB
*   **Por qué:** Registrar telemetría de bitrates y espectadores en bases de series de tiempo de forma eficiente.
*   **Cómo:** Enviar datos de descarga de fondo a bases de datos InfluxDB mediante peticiones asíncronas.

### 180. Analizador Predictivo de Horarios de Conexión
*   **Por qué:** Optimizar el sondeo del grabador reduciendo consultas en periodos históricamente inactivos del creador.
*   **Cómo:** Calcular distribuciones de probabilidad de conexión en base a registros históricos SQLite.

### 181. Generación Automática de Reportes Mensuales en HTML
*   **Por qué:** Analizar el inventario general y las descargas en un panel estético visual de escritorio.
*   **Cómo:** Generar reportes HTML con diagramas de barras utilizando consultas SQLite integradas.

### 182. Micro-Dashboard Web Embebido
*   **Por qué:** Controlar y previsualizar grabaciones desde cualquier dispositivo móvil conectado a la red local.
*   **Cómo:** Levantar servidor web ligero con Axum que devuelva información de uso del grabador en tiempo real.

### 183. Medición y Registro de Latencia de Servidores HLS
*   **Por qué:** Identificar si cortes en los videos son causados por saturación de red local o de CDNs remotas.
*   **Cómo:** Almacenar tiempos de respuesta HTTP de cada segmento para análisis de latencias del servidor de video.

### 184. Analíticas de Frecuencia de Palabras del Chat
*   **Por qué:** Detectar instantes clave en el video analizando cambios drásticos en el flujo del chat.
*   **Cómo:** Registrar estadísticas de palabras repetitivas en la sala para etiquetar puntos importantes.

### 185. Cálculo de Eficiencia de Almacenamiento
*   **Por qué:** Decidir con precisión qué calidad de grabación ofrece el mejor balance de compresión en disco.
*   **Cómo:** Correlacionar peso final con minutos grabados y guardar ratios de Megabytes/Minuto por resolución.

### 186. Categorización Dinámica de Modelos según Actividad
*   **Por qué:** Adaptar de forma autónoma el polling asignando menos recursos a modelos esporádicos.
*   **Cómo:** Agrupar modelos en perfiles de frecuencia en base a registros de actividad de SQLite.

### 187. Panel TUI Dedicado a Estadísticas de Red y Disco
*   **Por qué:** Visualizar históricos de velocidad y disco sin abandonar la terminal de comandos.
*   **Cómo:** Crear componentes gráficos sencillos en la TUI de Ratatui de forma dedicada al rendimiento.

### 188. Tabla Consolidada de Logs de Error
*   **Por qué:** Localizar problemas de forma reiterativa del sistema simple sin leer volcados de logs extensos.
*   **Cómo:** Guardar logs estructurados con códigos y descripciones de error en SQLite.

### 189. Calculadora de Costo de Almacenamiento Estimado
*   **Por qué:** Cuantificar el consumo financiero si se contrata hosting o nubes de pago por uso.
*   **Cómo:** Multiplicar Gigabytes descargados por tasas de coste local configurables por el usuario.

### 190. Registro de Picos de Velocidad de Descarga
*   **Por qué:** Identificar rendimientos de red máximos alcanzados en descargas simultáneas.
*   **Cómo:** Monitorizar transferencias de bytes en intervalos y guardar picos de velocidad por sesión.

### 191. Mapeo de Tendencias de Hashtags de los Creadores
*   **Por qué:** Conocer qué temáticas o hashtags generan mayor permanencia de espectadores.
*   **Cómo:** Guardar hashtags vigentes del directo en el JSON de metadatos al iniciar.

### 192. Copias de Seguridad del Historial (Backup / Restore)
*   **Por qué:** Evitar pérdidas de estadísticas del servidor al reinstalar o migrar de máquina.
*   **Cómo:** Proporcionar comandos sencillos para exportar y validar el archivo SQLite de la base de datos.

### 193. Geolocalización e Identificación de Servidores CDN
*   **Por qué:** Identificar si servidores de video de determinados países presentan tasas de caídas más de altas.
*   **Cómo:** Geolocalizar la IP CDN de HLS y registrar el proveedor en la base de datos al arrancar descargas.

### 194. Registro Estructurado de Micro-cortes y Reconexiones
*   **Por qué:** Evaluar la estabilidad real de la transmisión para saber si descartar grabaciones con fallas graves.
*   **Cómo:** Escribir marcas con detalles de tiempo y pérdidas de sincronía tras reconexiones.

### 195. Registro de Espectadores y Donantes Destacados (Top Tippers)
*   **Por qué:** Identificar a usuarios influyentes del chat asociados a directos grabados.
*   **Cómo:** Almacenar nombres y sumas del canal de alertas de propinas en tablas SQLite.

### 196. Registro Periódico de la Curva de Audiencia
*   **Por qué:** Visualizar curvas de atención de usuarios a lo largo del directo de forma retrospectiva.
*   **Cómo:** Guardar espectadores de sala cada 5 minutos en base de datos para graficar históricos.

### 197. Conector de Base de Datos PostgreSQL Alternativo
*   **Por qué:** Habilitar arquitecturas avanzadas con varios daemons sincronizados contra una BD común.
*   **Cómo:** Permitir alternar rusqlite por implementaciones PostgreSQL en la capa de persistencia de Rust.

### 198. Auditoría Interna de Cambios de Configuración
*   **Por qué:** Rastrear si errores técnicos recientes coinciden con modificaciones manuales de flags.
*   **Cómo:** Guardar históricos de cambios de configuraciones de variables en base de datos.

### 199. Sistema de Alertas de Modelos Inactivos
*   **Por qué:** Descartar de la lista de vigilancia a cuentas cerradas o inactivas de larga duración.
*   **Cómo:** Rastrear modelos sin actividad online por más de 60 días en SQLite y sugerir desactivarlos.

### 200. Editor de Highlights Automatizado Basado en Propinas
*   **Por qué:** Generar cortes y clips rápidos de los mejores momentos sin edición manual de video.
*   **Cómo:** Identificar picos de propinas en base de datos y lanzar FFmpeg para cortar fragmentos de 30s.

---

## 9. Autenticación, Cookies y CSRF Avanzado

### 201. Validación de cookies mediante endpoint de perfil
*   **Por qué:** Evita iniciar conexiones WebSocket con cookies de sesión expiradas.
*   **Cómo:** Hacer una petición GET ligera a `/api/messaging/preferences/` y verificar que retorne 200 y no redirija a la página de login.

### 202. Extracción de token CSRF desde el DOM inicial
*   **Por qué:** El token CSRF cambia periódicamente y se requiere para llamadas POST críticas de tipping y de follow.
*   **Cómo:** Hacer un GET a la landing de Chaturbate, parsear el HTML buscando `csrfmiddlewaretoken` en inputs ocultos y cargarlo en memoria.

### 203. Inyección dinámica del header X-CSRFToken en peticiones asíncronas
*   **Por qué:** La API interna del sitio rechaza llamadas POST que no contengan esta cabecera específica si provienen de AJAX.
*   **Cómo:** Extraer el token de la cookie `csrftoken` y añadirlo dinámicamente como header `X-CSRFToken` en peticiones REST.

### 204. Forzado estricto de idioma inglés en cookies para mapeo de API coherente
*   **Por qué:** Si el usuario está en un país de habla hispana, la plataforma redirige a subdominios como `es.chaturbate.com`, alterando el formato de las respuestas JSON.
*   **Cómo:** Fijar la cookie `language=en` y `en_subdomain=1` de forma estricta en el jar de cookies en cada inicio de sesión.

### 205. Simulación del header X-Requested-With para llamadas AJAX
*   **Por qué:** Muchos endpoints internos como `/follow/api/online_followed_rooms/` devuelven 403 si este header no está configurado como `XMLHttpRequest`.
*   **Cómo:** Añadir `X-Requested-With: XMLHttpRequest` por defecto en todos los clientes REST de infraestructura.

### 206. Manejo seguro de la cookie sbr (Session Binding Reference)
*   **Por qué:** La cookie `sbr` vincula la sesión con el fingerprint del navegador; su omisión puede activar alertas de seguridad y captcha en Cloudflare.
*   **Cómo:** Capturar la cookie `sbr` devuelta en el login exitoso y guardarla junto con la cookie de sesión `sessionid`.

### 207. Persistencia del User-Agent emparejado con la sesión
*   **Por qué:** Cloudflare detecta si una misma sesión con `sessionid` de repente cambia de User-Agent, provocando la invalidación inmediata de la cookie.
*   **Cómo:** Guardar el User-Agent utilizado al generar/extraer las cookies en el mismo archivo `cookies.toml` y reutilizarlo de forma obligatoria en cada conexión.

### 208. Soporte para cookie cf_clearance dinámica
*   **Por qué:** Permite saltarse los retos de Cloudflare si se dispone de una cookie cf_clearance generada recientemente en un navegador real.
*   **Cómo:** Permitir ingresar la cookie `cf_clearance` en la configuración TOML para inyectarla en el cliente HTTP del daemon.

### 209. Actualización asíncrona de cookies mediante pooling REST local
*   **Por qué:** Evitar tener que detener el daemon para actualizar cookies cuando expiran.
*   **Cómo:** Exponer un puerto local `/api/cookies` protegido que permita actualizar las cookies cargadas en memoria al vuelo.

### 210. Simulación de peticiones OPTIONS preliminares (CORS Preflight)
*   **Por qué:** Los WAF evalúan si el cliente realiza solicitudes previas OPTIONS para identificar bots simples y bloquear llamadas REST directas.
*   **Cómo:** Realizar una petición OPTIONS corta al endpoint antes de hacer un POST pesado para imitar el comportamiento del navegador.

### 211. Reintentos de autenticación con refresco automático de cookies desde navegador en red local
*   **Por qué:** Si la IP cambia, las cookies anteriores se invalidan de inmediato en los endpoints del servidor.
*   **Cómo:** Configurar un fallback que intente conectar al navegador local del usuario en la misma subred para obtener cookies frescas.

### 212. Detección automática de redirecciones de login fallidos
*   **Por qué:** Evitar loops de llamadas si las cookies expiraron y el servidor redirige continuamente a `/accounts/login/`.
*   **Cómo:** Configurar el cliente de reqwest para no seguir redirecciones (`redirect(Policy::none())`) y tratar las respuestas 302 como errores de autenticación explícitos.

### 213. Limpieza automatizada de cookies inválidas en base de datos
*   **Por qué:** Evitar reintentos de conexión con cookies corruptas o caducadas.
*   **Cómo:** Si una cookie retorna 403 o redirección reiterada, marcarla como "expirada" en SQLite para que no sea seleccionada de nuevo.

### 214. Soporte de múltiples cookies de sesión con prioridades
*   **Por qué:** Si una cuenta es baneada o limitada, el programa puede continuar descargando usando una cuenta secundaria.
*   **Cómo:** Definir un pool de cookies de sesión en el TOML de configuración y alternar entre ellas si hay fallos recurrentes.

### 215. Manejo del header Sec-Fetch-Dest para imitar comportamiento de navegación
*   **Por qué:** Cloudflare e firewalls WAF evalúan estas cabeceras modernas para clasificar el origen de la llamada.
*   **Cómo:** Añadir `Sec-Fetch-Dest: empty` en llamadas REST y `Sec-Fetch-Dest: document` al acceder a páginas HTML.

### 216. Cabecera Sec-Fetch-Mode adaptativa
*   **Por qué:** Validar el tipo de petición de red de acuerdo al estándar de los navegadores actuales.
*   **Cómo:** Usar `Sec-Fetch-Mode: cors` en llamadas asíncronas y `navigate` en accesos iniciales.

### 217. Cabecera Sec-Fetch-Site para simular peticiones del mismo origen (same-origin)
*   **Por qué:** Las llamadas a APIs internas de Chaturbate deben figurar como originadas desde el propio dominio de Chaturbate.
*   **Cómo:** Inyectar el header `Sec-Fetch-Site: same-origin` en todas las llamadas de la API interna.

### 218. Simulación del header Referer dinámico
*   **Por qué:** Los servidores de streaming bloquean descargas de HLS si el Referer no coincide con la URL de la sala del modelo.
*   **Cómo:** Configurar el Referer de forma dinámica como `https://chaturbate.com/{username}/` al solicitar los playlists `.m3u8`.

### 219. Evitar cabeceras automáticas de Rust en peticiones
*   **Por qué:** Cabeceras por defecto de Rust (ej. `User-Agent: reqwest/0.11`) revelan que es un bot al instante.
*   **Cómo:** Limpiar y deshabilitar los headers por defecto del cliente HTTP Rust en tiempo de inicialización.

### 220. Validación de consistencia de IP vs Cookie de sesión
*   **Por qué:** Si la cookie fue generada con una VPN y el daemon watch se ejecuta sin ella, la sesión se cerrará de inmediato.
*   **Cómo:** Comparar la IP pública de resolución de cookies con la IP pública de ejecución de `cbrec` antes de iniciar el polling.

### 221. Captura dinámica del token de Ably con cookies de sesión activas
*   **Por qué:** Conectarse al WebSocket de Ably requiere de un JWT autorizado que expira a las pocas horas.
*   **Cómo:** Realizar un POST a `/push_service/auth/` con las cookies cargadas para renovar el JWT de Ably automáticamente cuando expire.

### 222. Deshabilitar compresión gzip en depuraciones para evitar inconsistencias de TLS
*   **Por qué:** Algunos firewalls evalúan la aceptación de algoritmos de compresión para firmas de bot.
*   **Cómo:** Configurar de forma explícita el soporte de compresión (`gzip`, `deflate`, `br`) imitando exactamente la cabecera `Accept-Encoding` de Chrome.

### 223. Soporte para variables de entorno de cookies de sesión
*   **Por qué:** Facilitar el despliegue del grabador en entornos de contenedores (Docker) sin escribir archivos TOML de configuración físicos.
*   **Cómo:** Permitir que `cbrec` lea las cookies desde la variable de entorno `CBREC_SESSION_COOKIE`.

### 224. Ofuscación de trazas de cookies en los archivos de log
*   **Por qué:** Evitar la filtración accidental de las cookies de sesión en logs del sistema al compartir capturas o reportar errores.
*   **Cómo:** Implementar un filtro en la capa de logging que reemplace cadenas de cookies sensibles por `[REDACTED]`.

### 225. Carga de cookies mediante escaneo de código QR local
*   **Por qué:** Transferir la sesión activa desde el móvil al servidor de forma ágil sin usar cables o SSH pesado.
*   **Cómo:** Implementar un lector QR interactivo en la consola que reciba el payload cifrado de la sesión de cookies.

### 226. Detección de cierre de sesión por múltiples IPs (IP Ban / Session Lock)
*   **Por qué:** La plataforma puede cerrar la sesión si detecta actividad simultánea en dos IPs muy distantes.
*   **Cómo:** Detectar patrones de error del tipo "session locked" y pausar el daemon para evitar penalizaciones en la cuenta.

### 227. Generación automática del campo presence_id para auth de Ably
*   **Por qué:** La autenticación en `/push_service/auth/` requiere un identificador aleatorio de presencia único.
*   **Cómo:** Implementar un generador de strings alfanuméricos aleatorios de longitud coincidente en la capa de red.

### 228. Bypass de comprobaciones HTTP2 fingerprinting
*   **Por qué:** Firewalls de Cloudflare analizan las tramas y configuraciones iniciales de HTTP2 para detectar librerías de red Rust.
*   **Cómo:** Configurar de forma manual la ventana de inicio y parámetros de frames HTTP2 en el cliente `reqwest/hyper`.

### 229. Simulación de cookies de rastreo secundarias (Google Analytics, etc.)
*   **Por qué:** La ausencia total de cookies de analíticas delata un cliente de red automatizado.
*   **Cómo:** Almacenar cookies vacías o simuladas de analíticas comunes (`_ga`, `_gid`) en el jar de cookies local.

### 230. Forzado de resolución DNS local para dominios de Chaturbate
*   **Por qué:** Evitar que envenenamientos de DNS o bloqueos a nivel de ISP impidan conectar con los endpoints de la API.
*   **Cómo:** Permitir mapear IPs duras (hardcoded IPs) de servidores de Chaturbate en la sección de configuración del programa.

### 231. Renovación automática de la cookie cf_clearance via Puppeteer/Playwright
*   **Por qué:** Si Cloudflare incrementa la seguridad, se requerirá de un navegador real headless para resolver el bypass.
*   **Cómo:** Diseñar un plugin asíncrono que lance temporalmente una instancia de navegador headless para renovar `cf_clearance` y volver al daemon en Rust.

### 232. Gestión de cookies de sesión anónimas para ahorro de recursos
*   **Por qué:** No desgastar las cuentas de usuario de forma innecesaria en consultas que no requieren privilegios.
*   **Cómo:** Utilizar peticiones anónimas (sin cookies) para consultar el roomlist general y usar las cookies solo al solicitar enlaces HLS específicos.

### 233. Mapeo de expiración dinámica de cookies en base SQLite
*   **Por qué:** Disponer de una base centralizada para saber cuándo expira cada cuenta del pool de cookies.
*   **Cómo:** Guardar en la base de datos local un registro con la cookie, el User-Agent asociado y la fecha de caducidad estimada.

### 234. Almacenamiento seguro de cookies en el llavero de macOS (Keychain)
*   **Por qué:** Integración nativa y máxima seguridad en sistemas macOS.
*   **Cómo:** Utilizar la API de Keychain de macOS a través de crates en la capa de adaptadores para almacenar cookies.

### 235. Soporte de cookies en formato JSON nativo de Chrome DevTools
*   **Por qué:** Facilitar al usuario la exportación directa de cookies desde la pestaña Application del navegador Chrome.
*   **Cómo:** Implementar un parser que acepte archivos JSON con estructura de Array de objetos de cookies exportados directamente de DevTools.

### 236. Detección automática de bloqueos de IP a nivel de TCP
*   **Por qué:** Saber si el fallo de conexión es debido a un ban del servidor web de Chaturbate o un fallo de red local.
*   **Cómo:** Evaluar si la llamada devuelve un error de timeout TCP en lugar de un error HTTP normal y alertar consecuentemente.

### 237. Inyección de cookies de subdominio de afiliado para evitar detecciones
*   **Por qué:** El tráfico proveniente de subdominios específicos o enlaces con identificadores a veces tiene políticas anti-bot más laxas.
*   **Cómo:** Añadir cookies de afiliación simuladas en las llamadas REST iniciales.

### 238. Detección automática del cambio de la cookie de sesión por refresco del servidor
*   **Por qué:** La plataforma puede responder en los headers con un nuevo `Set-Cookie: sessionid=...` para renovar la sesión.
*   **Cómo:** Capturar de forma activa las cabeceras `Set-Cookie` de cada respuesta HTTP y actualizar el archivo `cookies.toml` local en caliente.

### 239. Soporte de proxies con autenticación integrada
*   **Por qué:** Permitir al usuario usar proxies privados estables que requieran usuario y contraseña.
*   **Cómo:** Configurar el cliente reqwest para aceptar proxies formateados como `http://user:pass@proxy_ip:port`.

### 240. Evasión de bloqueos mediante rotación de MTU de red
*   **Por qué:** Algunos firewalls bloquean paquetes TCP sospechosos de gran tamaño que son típicos de scripts de scraping.
*   **Cómo:** Permitir ajustar de forma fina el tamaño de paquete en la descarga de segmentos HLS.

### 241. Simulación de cabecera Accept-Language adaptativa
*   **Por qué:** La cabecera `Accept-Language` debe coincidir de forma lógica con el idioma de la cookie de sesión cargada.
*   **Cómo:** Si se fuerza el inglés en las cookies, forzar también `Accept-Language: en-US,en;q=0.9` en todos los headers.

### 442. Monitorización del estado de baneo de cuentas de usuario
*   **Por qué:** Si una cuenta de Chaturbate es suspendida por la plataforma, el grabador debe dejar de usarla inmediatamente.
*   **Cómo:** Detectar respuestas que contengan redirecciones o textos específicos indicando que la cuenta ha sido suspendida.

### 243. Desacoplamiento de cliente REST y cliente WebSocket en cookies
*   **Por qué:** Evitar que problemas de red en el WebSocket expiren o comprometan la sesión HTTP del cliente REST principal.
*   **Cómo:** Usar dos instancias separadas de jars de cookies en memoria para aislar los flujos de red.

### 244. Generación aleatoria de cabeceras Sec-Ch-Ua-Platform-Version
*   **Por qué:** Los WAF avanzados cruzan la versión de plataforma informada con la del User-Agent.
*   **Cómo:** Emparejar las versiones del sistema operativo simulado de forma lógica con los User-Agents rotados.

### 245. Soporte para autenticación de cookies via sockets Unix locales
*   **Por qué:** Permitir que scripts externos envíen cookies seguras al daemon sin exponer puertos de red TCP en la máquina.
*   **Cómo:** Levantar un socket Unix en `/tmp/cbrec.sock` exclusivamente para recibir comandos de configuración en caliente de cookies.

### 246. Simulación de cabecera Connection: keep-alive en HTTP1.1
*   **Por qué:** Mantener la conexión TCP abierta con el servidor para agilizar las peticiones y reducir handshakes TLS sospechosos.
*   **Cómo:** Configurar el cliente HTTP para forzar el reuso de sockets en todas las peticiones a endpoints de Chaturbate.

### 247. Validación y limpieza de cabeceras de depuración HTTP (Proxy Headers)
*   **Por qué:** Evitar que cabeceras inyectadas por proxies locales (ej. `X-Forwarded-For`) revelen la IP real del servidor de grabación.
*   **Cómo:** Limpiar de forma explícita todos los headers sospechosos antes de enviar las peticiones a la red externa.

### 248. Detección automática de redirecciones de geobloqueo a dominios locales
*   **Por qué:** Si Chaturbate intenta redirigir a un dominio específico de país debido a bloqueo legal (ej. en Alemania o Italia).
*   **Cómo:** Detectar intentos de redirección a dominios no estándar de Chaturbate y alertar de la necesidad de configurar proxies.

### 249. Rotación de perfiles de cookies en base a horas del día
*   **Por qué:** Simular que el usuario se conecta y desconecta de su cuenta de forma natural a lo largo de las 24 horas del día.
*   **Cómo:** Programar cambios de cookies de sesión activas a anónimas en periodos de inactividad de grabaciones.

### 250. Copia de seguridad encriptada del archivo de cookies al vuelo
*   **Por qué:** Evitar perder la sesión de cookies activa si el programa se apaga de golpe mientras escribe el archivo de configuración.
*   **Cómo:** Escribir en un archivo temporal `cookies.toml.tmp` y luego renombrarlo atómicamente a `cookies.toml`.

---

## 10. APIs REST - Descubrimiento y Búsqueda Detallada

### 251. Búsqueda de salas por términos específicos en `/api/ts/roomlist/room-list/?search={term}`
*   **Por qué:** Permitir buscar modelos que coincidan con tags o descripciones de show en tiempo real de forma dinámica.
*   **Cómo:** Implementar el comando `cbrec search {termino}` que consulte el endpoint con el query y dibuje los resultados en consola.

### 252. Filtrado de salas en show privado mediante query `private=true`
*   **Por qué:** Monitorizar o clasificar de forma masiva a todos los creadores que están en directo en modo privado.
*   **Cómo:** Pasar el parámetro `private=true` al GET de la lista de salas y clasificar el output de salida.

### 253. Búsqueda de salas ocultas mediante filtros de la API
*   **Por qué:** Descubrir y grabar salas que están activas pero no figuran en el index general público.
*   **Cómo:** Realizar peticiones al roomlist agregando las flags `hidden=true&private=false` en los queries.

### 254. Paginación automatizada del roomlist mediante limit y offset
*   **Por qué:** Obtener listados completos de cientos de salas en directo sin perder registros por límites de respuesta del servidor.
*   **Cómo:** Implementar un bucle asíncrono que incremente el parámetro `offset` en pasos del parámetro `limit` hasta que retorne un array vacío.

### 255. Obtención masiva de tags disponibles vía `/api/ts/roomlist/all-tags/`
*   **Por qué:** Conocer qué temáticas o hashtags están activos en la plataforma para ofrecer autocompletado en búsquedas.
*   **Cómo:** Consultar el endpoint de tags con límites dinámicos y presentarlos en el prompt de la CLI.

### 256. Búsqueda rápida de hashtags más populares vía `/api/ts/hashtags/top_tags/`
*   **Por qué:** Conocer cuáles son las tendencias actuales en vivo para sugerir grabaciones temáticas.
*   **Cómo:** Consultar el endpoint enviando el parámetro `count` con el número de hashtags deseado.

### 257. Validación de hashtags antes de iniciar búsquedas
*   **Por qué:** Evitar llamadas infructuosas con hashtags que no existen o están prohibidos por el sitio.
*   **Cómo:** Enviar los tags al endpoint `/api/ts/hashtags/approved_from_tags_list/?tags={tag}` y validar la respuesta positiva.

### 258. Descubrimiento de modelos similares con `/api/more_like/{username}/`
*   **Por qué:** Sugerir grabaciones alternativas si un modelo específico está offline de forma de persistente.
*   **Cómo:** Obtener la lista de usuarios similares e iniciar monitoreo secundario si el principal no está disponible.

### 259. Petición periódica de roomlist con límites optimizados
*   **Por qué:** Evitar sobrecargas de red solicitando un número de elementos (limit) acorde a la velocidad de descarga.
*   **Cómo:** Ajustar dinámicamente el parámetro `limit` (ej. entre 10 y 100) de acuerdo al número de hilos de grabación activos.

### 260. Mapeo del campo display_age para filtros de grabación
*   **Por qué:** Permitir al usuario grabar solo modelos dentro de ciertos rangos de edad (ej. mayores de 21).
*   **Cómo:** Evaluar el campo `display_age` del JSON de retorno antes de disparar el inicio de la grabación.

### 261. Filtrado por género en base al campo `gender`
*   **Por qué:** Permitir automatizar grabaciones filtradas por género del creador (ej. f, m, c, s).
*   **Cómo:** Implementar filtros en la CLI del daemon (ej. `--gender f`) y descartar de la descarga a creadores que no coincidan.

### 262. Registro de localización geográfica de las salas en SQLite
*   **Por qué:** Disponer de información analítica de dónde transmiten más los creadores monitorizados.
*   **Cómo:** Mapear la clave `location` del JSON de la sala y guardarla en la tabla SQLite de estadísticas de la grabación.

### 263. Detección automática del estado "new" del modelo
*   **Por qué:** Priorizar la grabación de creadores nuevos en la plataforma, ya que sus shows iniciales suelen ser muy activos.
*   **Cómo:** Evaluar el campo `is_new` del JSON de la sala para priorizar el hilo de descarga.

### 264. Monitoreo del total de seguidores vía `num_followers`
*   **Por qué:** Analizar el crecimiento en popularidad de los creadores a lo largo de los meses.
*   **Cómo:** Guardar el número de seguidores obtenido de la API al iniciar cada sesión de grabación en la base SQLite.

### 265. Registro del timestamp exacto de inicio de transmisión `start_timestamp`
*   **Por qué:** Calcular la duración real del directo del modelo y sincronizarla con el tiempo de descarga.
*   **Cómo:** Guardar la marca de tiempo `start_timestamp` en los metadatos JSON y compararla con el timestamp del primer fragmento de video.

### 266. Validación del estado de show gratuito vs de pago (is_gaming, spy_show_price)
*   **Por qué:** Tomar decisiones sobre si vale la pena grabar un show si este pasa a ser de pago por eventos especiales.
*   **Cómo:** Analizar campos como `spy_show_price` o `private_price` en el listado de salas antes de conectar.

### 267. Identificación del label de promoción del stream (label)
*   **Por qué:** Saber si el modelo está promocionando su sala ("promoted") para predecir picos de audiencia y propinas.
*   **Cómo:** Leer el campo `label` del JSON de la sala y guardarlo en la base de datos de logs de grabación.

### 268. Caching dinámico del JSON del roomlist para búsquedas concurrentes
*   **Por qué:** Evitar bloqueos de IP si se realizan múltiples comandos de búsqueda CLI en un breve lapso de tiempo.
*   **Cómo:** Guardar la respuesta del roomlist en un archivo de caché temporal `/tmp/cbrec_roomlist.json` por 15 segundos.

### 269. Alerta de cambio drástico en espectadores de sala (`num_users`)
*   **Por qué:** Detectar si el stream ha sido "raideado" o si está experimentando un pico masivo de interés.
*   **Cómo:** Guardar en memoria el `num_users` previo y notificar si se duplica o triplica entre ciclos del daemon.

### 270. Filtro de búsqueda por prefijos de nombres de modelos
*   **Por qué:** Buscar de forma rápida variantes de nombres de modelos conocidos.
*   **Cómo:** Implementar búsqueda con comodines en el comando de búsqueda de la CLI (ej. `cbrec search alice*`).

### 271. Detección automática del tipo de transmisión (gaming, show, etc.)
*   **Por qué:** Agrupar videos grabados en carpetas temáticas correspondientes.
*   **Cómo:** Evaluar si el campo `is_gaming` es true y derivar el almacenamiento del video a una subcarpeta `gaming/`.

### 272. Clasificación de salas por tipo de promoción (`source_name`)
*   **Por qué:** Investigar si las promociones pagas de la plataforma influyen en la calidad del stream del modelo.
*   **Cómo:** Guardar el parámetro `source_name` (ej. "df", "rc", "pr") en las tablas SQLite de metadatos.

### 273. Descarga masiva de miniaturas de salas activas (Thumbs Grid)
*   **Por qué:** Disponer de una grilla de imágenes de todos los modelos en watch para previsualización rápida.
*   **Cómo:** Consultar el roomlist, extraer las URLs de las imágenes de la clave `img` y descargarlas de forma de concurrente a una carpeta temporal.

### 274. Sincronización de lista de modelos recomendados con watch daemon
*   **Por qué:** Permitir auto-descubrimiento y grabación de nuevos creadores sin intervención del usuario.
*   **Cómo:** Añadir la flag `cbrec watch --auto-recommend` que añada a la lista de descargas modelos similares a los favoritos usando `/api/more_like/`.

### 275. Monitoreo de horarios de desconexión mediante API de salas
*   **Por qué:** Estimar la duración típica de las transmisiones de los modelos monitorizados.
*   **Cómo:** Registrar la hora en la que el modelo desaparece de las respuestas del roomlist y calcular la diferencia temporal.

### 276. Detección del tipo de show actual mediante roomlist
*   **Por qué:** Clasificar los shows en base a si son públicos, privados o de club de fans.
*   **Cómo:** Leer la propiedad `current_show` del JSON de la sala y guardar la etiqueta al guardar el archivo de video.

### 277. Búsqueda de salas en lote de múltiples hashtags
*   **Por qué:** Monitorear transmisiones activas asociadas a varias categorías temáticas a la vez.
*   **Cómo:** Iterar el endpoint de roomlist enviando los hashtags separados por coma en el parámetro de consulta.

### 278. Alerta visual de nuevo modelo "New" online
*   **Por qué:** Identificar rápidamente creadores nuevos que inician transmisión en la CLI.
*   **Cómo:** Mostrar un distintivo de color verde `[NUEVO]` en las notificaciones y logs de la CLI si `is_new` es verdadero.

### 279. Filtrado de modelos con contraseñas en el comando search
*   **Por qué:** Evitar listar en las búsquedas interactivas salas que no se pueden visualizar de forma pública.
*   **Cómo:** Omitir de la tabla de resultados de búsqueda aquellas salas que tengan `has_password: true`.

### 280. Análisis estadístico de edades de modelos activos
*   **Por qué:** Conocer demográficamente qué rangos de edad son los más activos en las listas de monitoreo.
*   **Cómo:** Procesar las edades del roomlist y generar gráficos de distribución en los reportes HTML.

### 281. Monitoreo de la presencia de banners de promoción en la sala
*   **Por qué:** Identificar si el modelo está en un show especial patrocinado por el sitio.
*   **Cómo:** Evaluar el campo `label` en búsqueda de palabras clave como `promoted` u otras variantes.

### 282. Peticiones concurrentes limitadas del roomlist para optimizar red
*   **Por qué:** Evitar ser bloqueado por la API por realizar llamadas concurrentes masivas.
*   **Cómo:** Usar un semáforo de red (`tokio::sync::Semaphore`) para limitar las consultas de APIs REST a un máximo de 2 en paralelo.

### 283. Mapeo del origen de la imagen de miniatura del modelo
*   **Por qué:** Asegurar que el dominio de CDN de miniaturas de la plataforma es accesible para la previsualización.
*   **Cómo:** Validar la URL del campo `img` y reportar si los dominios de miniaturas (`thumb.live.mmcdn.com`) cambian.

### 284. Búsqueda de modelos por localización geográfica exacta
*   **Por qué:** Encontrar creadores que transmitan desde un país específico de interés para el usuario.
*   **Cómo:** Filtrar las respuestas del roomlist en base al string contenido en la propiedad `location`.

### 285. Guardado del JSON completo de metadatos de la sala por sesión
*   **Por qué:** Disponer de una copia exacta del estado de la sala al momento del inicio de la grabación para auditorías.
*   **Cómo:** Serializar y escribir el objeto JSON de la sala directamente en el log de metadatos del video.

### 286. Alerta de cambio de tags a mitad de transmisión
*   **Por qué:** Saber si el modelo ha cambiado la temática o las actividades del show durante el directo.
*   **Cómo:** Consultar el roomlist periódicamente y comparar la lista de tags actual con la capturada al inicio.

### 287. Exclusión opcional de modelos promocionados en las búsquedas
*   **Por qué:** Enfocar los resultados de búsqueda en modelos orgánicos en lugar de publicidad del sitio.
*   **Cómo:** Añadir la flag `--exclude-promoted` al comando `cbrec search` para filtrar salas patrocinadas.

### 288. Notificación si un modelo de alta prioridad cambia de tags
*   **Por qué:** Alertar al usuario si el creador empieza una actividad específica de su interés.
*   **Cómo:** Si un tag de interés (ej. "music") aparece en el array de tags del modelo en watch, enviar una alerta inmediata.

### 289. Medidor de tiempo de carga de la API de salas
*   **Por qué:** Diagnosticar problemas de latencia con los servidores REST de Chaturbate.
*   **Cómo:** Cronometrar la llamada HTTP a `/api/ts/roomlist/room-list/` y guardarla en la telemetría del daemon.

### 290. Filtro de búsqueda interactivo por número de seguidores mínimo
*   **Por qué:** Reducir los resultados de búsquedas en la consola omitiendo canales muy pequeños.
*   **Cómo:** Flag `--min-followers {N}` en el subcomando search para filtrar salas de salida.

### 291. Detección automática del cambio de miniatura del modelo
*   **Por qué:** Capturar nuevas imágenes de portada si el modelo las actualiza durante el directo.
*   **Cómo:** Comprobar si la URL del campo `img` o su hash cambia durante la transmisión y descargar la nueva miniatura.

### 292. Búsqueda difusa en el listado de tags locales
*   **Por qué:** Ayudar a encontrar tags válidos si el usuario comete errores tipográficos en la consola.
*   **Cómo:** Usar algoritmos de distancia Levenshtein sobre la lista de tags descargados de `/all-tags/`.

### 293. Guardado del historial de tags por modelo
*   **Por qué:** Rastrear cuáles son las categorías que más utiliza cada creador en el tiempo.
*   **Cómo:** Almacenar la lista de tags en una tabla de relación `model_tags` en SQLite en cada directo grabado.

### 294. Detección de salas privadas gratuitas (Free Private Shows)
*   **Por qué:** Aprovechar grabaciones de shows privados que permiten visualización gratuita bajo condiciones.
*   **Cómo:** Validar si `private_price` es cero y el show está clasificado como privado en la API de salas.

### 295. Exportación del roomlist a formato HTML local
*   **Por qué:** Disponer de una grilla visual de salas activas local fuera del navegador web.
*   **Cómo:** Escribir un archivo temporal HTML con las miniaturas de las salas y enlaces directos al reproductor.

### 296. Búsqueda de salas en vivo usando listas de texto plano externas
*   **Por qué:** Facilitar el monitoreo masivo alimentando el buscador mediante archivos externos.
*   **Cómo:** Leer un archivo de texto con tags de búsqueda y procesar consultas en lote contra la API.

### 297. Alerta de velocidad de crecimiento de espectadores (Trending Rooms)
*   **Por qué:** Grabar salas que se están volviendo virales en tiempo real de forma automática.
*   **Cómo:** Identificar si la tasa de crecimiento de espectadores en una sala supera un porcentaje determinado por minuto.

### 298. Detección de streams caídos por copyright o baneo a mitad de show
*   **Por qué:** Detener grabaciones de forma limpia si la sala es suspendida de golpe por la plataforma.
*   **Cómo:** Si el roomlist indica que la sala fue eliminada de forma abrupta y el stream HLS arroja 404 de inmediato.

### 299. Filtrado de búsquedas por países de origen
*   **Por qué:** Encontrar contenido generado desde regiones específicas compatibles con el idioma del usuario.
*   **Cómo:** Analizar strings comunes en el campo `location` para clasificar procedencias (ej. "Colombia", "Spain").

### 300. Caching de las respuestas del all-tags en memoria para optimizar búsquedas
*   **Por qué:** El endpoint de todos los tags es pesado y cambia poco a lo largo del día.
*   **Cómo:** Mantener la respuesta del all-tags en una caché estática en memoria con un tiempo de vida (TTL) de 12 horas.

---

## 11. APIs REST - Contexto del Broadcaster y Aplicaciones

### 301. Monitoreo de widgets de metas con `/api/panel_context/{username}/`
*   **Por qué:** Registrar el avance de la meta del modelo de forma estructurada a lo largo del video.
*   **Cómo:** Consultar el widget JSON periódicamente y guardar los valores de metas (ej. "Tip Goal") en la base de datos.

### 302. Captura del bio y perfil del modelo con `/api/biocontext/{username}/`
*   **Por qué:** Almacenar información de redes sociales y descripción del modelo junto a sus grabaciones.
*   **Cómo:** Hacer un GET al biocontext al iniciar la grabación y volcar la descripción al archivo de metadatos.

### 303. Consulta de estado del viewer con `/api/ts/chatmessages/user_info/{username}/?room={room}`
*   **Por qué:** Validar los permisos reales de la sesión (ej. si somos moderadores o miembros del fanclub) en la sala.
*   **Cómo:** Realizar la consulta usando las cookies de sesión y guardar las flags de privilegios del usuario.

### 304. Detección de juegos activos mediante `/api/ts/games/current/room/{username}`
*   **Por qué:** Identificar dinámicamente si el modelo está interactuando con retos y juegos de tokens.
*   **Cómo:** Guardar el nombre y detalles del juego activo en los logs de la sesión de grabación.

### 305. Consulta de aplicaciones interactivas con `/api/public/asp/broadcast/applist/{broadcaster_uid}/`
*   **Por qué:** Conocer el uso de juguetes de control interactivo por parte del modelo durante la transmisión.
*   **Cómo:** Consultar la lista de apps usando el UID resuelto del creador y registrar la telemetría en SQLite.

### 306. Consulta de comandos rápidos con `/api/public/asp/shortcuts/{broadcaster_uid}/`
*   **Por qué:** Conocer los comandos de chat abreviados y menús rápidos configurados por el modelo.
*   **Cómo:** Hacer GET al endpoint de shortcuts y archivar los atajos disponibles en los metadatos.

### 307. Consulta del costo de promoción en `/promotion/api/promote_price/?slug={username}`
*   **Por qué:** Rastrear la inversión que realiza el modelo en publicitar su show en la plataforma.
*   **Cómo:** Consultar el endpoint de promoción al arrancar y guardar el coste estimado en los logs.

### 308. Alerta si el modelo alcanza el objetivo de la meta (Goal Completed)
*   **Por qué:** Registrar eventos clave del show en la línea de tiempo del video grabado.
*   **Cómo:** Comparar periódicamente los valores del panel_context y notificar si el valor actual alcanza la meta.

### 309. Registro de la propina más alta (Highest Tip) del directo
*   **Por qué:** Conocer la mayor contribución realizada durante el show y quién la envió.
*   **Cómo:** Extraer el usuario y monto de la clave `row2_value` en el JSON del panel_context y guardarlo en SQLite.

### 310. Registro de la última propina recibida (Latest Tip Received)
*   **Por qué:** Seguir el flujo de interacción de los donantes con el creador desde los metadatos.
*   **Cómo:** Mapear la clave `row3_value` del panel_context y guardarla en la tabla de transacciones de la sesión.

### 311. Caching de biocontext de modelos para evitar bloqueos
*   **Por qué:** Los datos biográficos del creador cambian poco y no deben consultarse en cada inicio de grabación.
*   **Cómo:** Guardar el bio del modelo en SQLite y actualizarlo únicamente cada 7 días.

### 312. Detección del estado de suscripción del fan club del viewer
*   **Por qué:** Activar de forma automática grabaciones de mayor calidad si la sesión de cookies tiene acceso al fan club.
*   **Cómo:** Validar si la propiedad `in_fanclub` del user_info es true para seleccionar el playlist premium.

### 313. Detección de privilegios de moderador del viewer
*   **Por qué:** Habilitar comandos especiales en la TUI de control si el usuario es moderador de la sala grabada.
*   **Cómo:** Leer la clave `is_mod` devuelta por el endpoint de user_info del usuario en la sala.

### 314. Consulta automática de saldo de tokens del viewer en la sala
*   **Por qué:** Alertar en la TUI si nos estamos quedando sin saldo de tokens en la sesión activa.
*   **Cómo:** Extraer el balance de tokens de la clave correspondiente de user_info del viewer.

### 315. Alerta de inicio de un juego interactivo en la sala
*   **Por qué:** Notificar al usuario si el creador inicia un juego interactivo que requiera atención.
*   **Cómo:** Detectar cambios en la respuesta del endpoint de juegos del modelo y enviar una alerta push.

### 316. Mapeo de atajos de comandos del teclado del broadcaster
*   **Por qué:** Registrar la lista de shortcuts habilitados por el modelo para entender las interacciones del chat.
*   **Cómo:** Parsear las respuestas del endpoint `/shortcuts/` y guardarlas en formato de diccionario local.

### 317. Valiadción de disponibilidad del endpoint de shortcuts
*   **Por qué:** Evitar registrar errores 404 en el log si el modelo no tiene configurado ningún shortcut.
*   **Cómo:** Interceptar respuestas 404 del endpoint de shortcuts y deshabilitar consultas posteriores en esa sesión.

### 318. Registro del tipo de template del widget de metas
*   **Por qué:** Documentar el estilo de presentación de metas que prefiere utilizar el creador.
*   **Cómo:** Guardar el string de la clave `template` devuelto por `/api/panel_context/`.

### 319. Análisis de evolución del tip goal en la sesión
*   **Por qué:** Evaluar la tasa de éxito de recaudación de tokens del modelo a lo largo de las horas.
*   **Cómo:** Registrar los valores de meta actual en la base de datos cada 10 minutos y generar una curva de recaudación.

### 320. Detección de juguetes Lovense activos en applist
*   **Por qué:** Etiquetar grabaciones en las que se usaron juguetes de control remoto para facilitar búsquedas.
*   **Cómo:** Buscar la palabra "lovense" o similares dentro del array de apps activas del endpoint de applist.

### 321. Notificación si el modelo activa una app específica
*   **Por qué:** Alertar al usuario si el creador inicia una aplicación interactiva de su preferencia.
*   **Cómo:** Comparar la lista de apps activas en ciclos de 5 minutos y notificar al usuario sobre nuevas incorporaciones.

### 322. Mapeo del UID del modelo en base al biocontext
*   **Por qué:** Disponer del UID para realizar consultas asíncronas seguras en otros endpoints de infraestructura.
*   **Cómo:** Si el biocontext retorna el UID del creador, almacenarlo de forma de persistente en el mapa de modelos en SQLite.

### 323. Descarga del avatar del modelo desde el biocontext
*   **Por qué:** Mostrar la foto de perfil del modelo en las notificaciones del sistema operativo y en la TUI.
*   **Cómo:** Extraer la URL de la imagen del perfil en el biocontext y guardarla en la caché local de imágenes.

### 324. Análisis de tags biográficos del creador
*   **Por qué:** Catalogar el estilo del creador más allá de los hashtags temporales de la sala.
*   **Cómo:** Parsear las secciones del bio buscando etiquetas fijas de descripción del modelo.

### 325. Detección de redirección del biocontext por deshabilitación de cuenta
*   **Por qué:** Identificar de forma temprana si la cuenta del creador ha sido borrada o suspendida por la web.
*   **Cómo:** Tratar respuestas 302 del biocontext que redirijan a páginas de error de cuenta como fallos de disponibilidad.

### 326. Detección de estado de transmisión silenciosa (Muted Broadcaster)
*   **Por qué:** Alertar al usuario si el creador transmite sin audio según configuraciones del bio.
*   **Cómo:** Buscar marcas de advertencia de audio desactivado en las opciones de contexto del broadcaster.

### 327. Registro del costo de spy show configurado por el modelo
*   **Por qué:** Documentar los precios de visualización de pago del creador para estadísticas.
*   **Cómo:** Extraer y guardar la propiedad del coste de show espía desde el user_info de la sala.

### 328. Registro de cambios de metas a mitad de directo
*   **Por qué:** Saber si el modelo ha definido una nueva meta de tokens tras completar la anterior.
*   **Cómo:** Detectar si el valor máximo de la meta en el panel_context cambia (ej. de 1000 a 2000) y guardar el evento.

### 329. Mapeo de shortcuts de chat a comandos del sistema
*   **Por qué:** Documentar las reglas de chat configuradas en la sala del modelo.
*   **Cómo:** Guardar los textos de atajos y sus descripciones en formato JSON estructurado.

### 330. Análisis del nivel de generosidad de la sala (Tipping Level)
*   **Por qué:** Conocer si la sala está en un periodo de alta generosidad de propinas.
*   **Cómo:** Medir la frecuencia de actualizaciones en el widget de metas y clasificar el estado de la sala.

### 331. Caching del estado de user_info del viewer en memoria
*   **Por qué:** Evitar peticiones redundantes de verificación de estado durante la grabación activa.
*   **Cómo:** Mantener en memoria el rol de usuario por la duración de la sesión del grabador de ese modelo.

### 332. Notificación si la meta de propinas está cerca de completarse (Goal Alert)
*   **Por qué:** Conectar para ver en directo el clímax del show cuando la meta está al 95%.
*   **Cómo:** Enviar alerta push si el valor actual de tokens de la meta supera el 95% del total requerido.

### 333. Detección del uso de apps de chat automatizadas (Bots de chat)
*   **Por qué:** Identificar si la sala utiliza moderadores robotizados para interactuar con los espectadores.
*   **Cómo:** Buscar nombres de bots comunes en la lista de apps activas de la sala.

### 334. Registro de la tasa de tokens por minuto del directo
*   **Por qué:** Evaluar cuantitativamente la rentabilidad y actividad del show grabado.
*   **Cómo:** Calcular la diferencia de tokens de la meta en intervalos de 1 minuto y guardar el promedio en SQLite.

### 335. Análisis de redes sociales vinculadas en el biocontext
*   **Por qué:** Facilitar al usuario enlaces rápidos a Twitter/Instagram del creador desde el reporte HTML.
*   **Cómo:** Buscar y extraer URLs de redes sociales dentro del texto del bio del modelo.

### 336. Detección de shows conjuntos (Couples shows)
*   **Por qué:** Etiquetar grabaciones donde participan múltiples modelos a la vez.
*   **Cómo:** Buscar menciones a otros usuarios en el bio o títulos de meta de la sala.

### 337. Alerta si el modelo desactiva una aplicación interactiva
*   **Por qué:** Documentar cambios de ritmo en las actividades del show del creador.
*   **Cómo:** Notificar en los logs del sistema si una app de Lovense desaparece del endpoint de applist.

### 338. Registro de la meta de tokens promedio de las grabaciones del modelo
*   **Por qué:** Conocer cuáles son las metas financieras habituales del creador monitorizado.
*   **Cómo:** Calcular la media de los valores máximos de metas de todas las grabaciones del creador en SQLite.

### 339. Validación de acceso a salas de Fan Club exclusivas
*   **Por qué:** Evitar descargas vacías de salas de club de fans si la cuenta no cuenta con suscripción activa.
*   **Cómo:** Consultar el user_info del viewer; si `in_fanclub` es false y el show es exclusivo, abortar de forma de segura.

### 340. Notificación si el viewer es promovido a moderador de la sala
*   **Por qué:** Informar al usuario en consola si el modelo le ha otorgado permisos de moderación en directo.
*   **Cómo:** Comparar el campo `is_mod` anterior y actual del user_info y mostrar un banner destacado.

### 341. Registro de shortcuts más habituales de la sala
*   **Por qué:** Documentar las palabras clave que activan acciones de juguetes interactivos en la sala.
*   **Cómo:** Guardar los comandos de shortcuts que coincidan con términos típicos de Lovense.

### 342. Detección de cambios de tarifa de show privado en caliente
*   **Por qué:** Documentar si el modelo incrementa el precio de los tokens del show privado durante el directo.
*   **Cómo:** Monitorizar cambios en la propiedad `private_price` del user_info y guardar el registro.

### 343. Alerta si el modelo inicia una transmisión desde una nueva localización
*   **Por qué:** Saber si el creador está de viaje o transmitiendo desde un entorno diferente al habitual.
*   **Cómo:** Comparar la localización geográfica del biocontext con la grabada en la base SQLite y alertar.

### 344. Registro de cambios de plantilla de metas
*   **Por qué:** Analizar si el modelo alterna entre diferentes tipos de widgets de objetivos.
*   **Cómo:** Registrar variaciones en el campo `name` o `template` de la respuesta del panel_context.

### 345. Mapeo de shortcuts de chat a menús de propinas
*   **Por qué:** Conocer qué combinaciones de palabras corresponden a precios de propinas en la sala.
*   **Cómo:** Correlacionar el texto de shortcuts con los ítems del menú de propinas en los metadatos.

### 346. Análisis de popularidad del modelo en base a seguidores históricos
*   **Por qué:** Graficar la evolución y el crecimiento de la popularidad del creador a lo largo de las semanas.
*   **Cómo:** Almacenar el campo de seguidores del biocontext en una tabla SQLite de serie de tiempo semanal.

### 347. Detección de la categoría de videojuego activa
*   **Por qué:** Registrar si la transmisión estuvo centrada en videojuegos durante periodos de tiempo.
*   **Cómo:** Consultar el endpoint de juegos y confirmar que la categoría corresponde a videojuegos conocidos.

### 348. Alerta si el modelo inicia show interactivo sin juguetes conectados
*   **Por qué:** Advertir si el show ha perdido interactividad técnica por problemas de conexión del modelo.
*   **Cómo:** Si el título menciona juguetes interactivos pero la applist de apps activas está vacía por más de 15 minutos.

### 349. Guardado de descripción de perfil de texto alternativo en SQLite
*   **Por qué:** Conservar la descripción textual del perfil del modelo por si es modificada o borrada en el futuro.
*   **Cómo:** Volcar el texto limpio de formato del bio al registro SQLite del modelo.

### 350. Caching dinámico de los shortcuts de la sala
*   **Por qué:** Optimizar red reduciendo llamadas a shortcuts que no varían durante la sesión de grabación.
*   **Cómo:** Guardar en memoria los shortcuts capturados al inicio y no volver a consultarlos hasta el próximo directo.

---

## 12. APIs REST - Chat, Mensajería y PMs

### 351. Consulta de opciones de renderizado de chat en `/api/ts/chat/message-render-options/`
*   **Por qué:** Configurar la generación de subtítulos de chat con fuentes y colores idénticos a los del sitio original.
*   **Cómo:** Hacer un GET al endpoint de renderizado al iniciar y pasar las opciones de color de fuente al generador SRT/ASS.

### 352. Exclusión automática de usuarios bloqueados mediante `/api/ts/chat/ignored-users/`
*   **Por qué:** Evitar guardar mensajes de spam o acosadores en el archivo de subtítulos de chat de la grabación.
*   **Cómo:** Consultar la lista de ignorados y filtrar los mensajes de esos nombres de usuario al escribir el subtítulo.

### 353. Reporte de calidad de reproducción con `/api/ts/chat/send-player-quality/`
*   **Por qué:** Evitar llamar la atención de los servidores simulando el reporte de calidad técnica que envía el reproductor web.
*   **Cómo:** Realizar un POST periódico a `/api/ts/chat/send-player-quality/` con la calidad (ej. "1080p") y el nombre de la sala.

### 354. Descarga de mensajes privados (DMs) en `/api/ts/chatmessages/pm_list/{username}/`
*   **Por qué:** Registrar las conversaciones de DMs asociadas a la sesión de grabación para usuarios autorizados.
*   **Cómo:** Consultar el pm_list enviando parámetros de offset y guardar las conversaciones en un log separado de DMs.

### 355. Descarga de imágenes del chat mediante `/api/ts/chatmessages/media/`
*   **Por qué:** Conservar las imágenes compartidas en el chat de la sala durante la transmisión del modelo.
*   **Cómo:** Consultar el endpoint de media del chat, extraer los enlaces de imágenes y descargarlas a la carpeta de adjuntos del video.

### 356. Obtención de la lista completa de usuarios en sala en `/api/getchatuserlist/`
*   **Por qué:** Documentar la lista de espectadores de la sala durante la grabación para análisis de audiencia.
*   **Cómo:** Consultar `/api/getchatuserlist/` con ordenamiento por tokens y guardarla en los logs JSON secundarios.

### 357. Guardado de notas personales del usuario sobre modelos en `/api/notes/usernames/`
*   **Por qué:** Clasificar grabaciones o aplicar reglas de watch basadas en notas de texto que el usuario asignó en la web.
*   **Cómo:** Consultar el endpoint de notas, asociarlas a los nombres de modelos en SQLite y aplicar reglas en base a ellas.

### 358. Consulta del conteo de mensajes privados sin leer en `/api/messaging/unread/`
*   **Por qué:** Alertar en el dashboard del grabador si la cuenta del usuario tiene mensajes privados sin leer.
*   **Cómo:** Realizar consultas al endpoint de unread y mostrar un contador destacado en la interfaz TUI.

### 359. Consulta del perfil de mensajería del modelo con `/api/messaging/profile/{username}/`
*   **Por qué:** Obtener detalles específicos de la bandeja de entrada del creador antes de enviar mensajes automáticos.
*   **Cómo:** Hacer GET al perfil de mensajería del modelo y registrar sus límites y preferencias de DMs.

### 360. Consulta de preferencias de mensajería en `/api/messaging/preferences/`
*   **Por qué:** Validar los límites y reglas de recepción de mensajes configurados en nuestra propia cuenta de usuario.
*   **Cómo:** Obtener y mostrar las preferencias de mensajería del usuario en las opciones de configuración de la CLI.

### 361. Envío automatizado de mensajes de chat con `/push_service/publish_chat_message_live/`
*   **Por qué:** Permitir al usuario enviar saludos o comandos al chat de la sala directamente desde la consola CLI de Rust.
*   **Cómo:** Realizar un POST con el token CSRF, el mensaje y el nombre de la sala al endpoint de envío de mensajes.

### 362. Notificación de presencia en sala en `/push_service/room_user_count/{username}/`
*   **Por qué:** Simular la presencia del viewer en la sala para que el modelo nos cuente como espectador activo y no nos desconecte.
*   **Cómo:** Hacer POST periódicos enviando el `presence_id` generado para registrar la actividad de la sesión del visor.

### 363. Alerta si un moderador silencia o banea a un usuario en el chat
*   **Por qué:** Documentar eventos de conflicto o moderación activa de la sala en el log del video.
*   **Cómo:** Detectar eventos de baneo en el chat y escribir la marca de tiempo de la sanción en la base de datos.

### 364. Filtrado de mensajes del bot del modelo en los subtítulos
*   **Por qué:** Mantener los subtítulos limpios de spam reiterativo de anuncios del bot del modelo.
*   **Cómo:** Identificar mensajes de usuarios que tengan configurados roles de bot o textos repetitivos de menús y omitirlos.

### 365. Detección del color asignado al viewer en el chat
*   **Por qué:** Generar subtítulos de chat donde nuestro propio usuario figure con el color exacto que verían otros.
*   **Cómo:** Leer el color de usuario asignado en las opciones de renderizado de la API de chat.

### 366. Caching de la lista de usuarios bloqueados en la sala
*   **Por qué:** Optimizar rendimiento evitando filtrar mensajes contra una API externa en cada línea del chat.
*   **Cómo:** Cargar el listado de usuarios ignorados al arrancar y mantenerlo en una estructura de conjunto (`HashSet`) en memoria.

### 367. Notificación si recibimos un mensaje privado (DM) del modelo durante la grabación
*   **Por qué:** Alertar al usuario para que interactúe si el modelo le escribe de forma directa mientras graba.
*   **Cómo:** Monitorear el pm_list del modelo y disparar una notificación de alta prioridad si entra un mensaje nuevo de su parte.

### 368. Registro del número de moderadores conectados en la sala
*   **Por qué:** Conocer el nivel de control y moderación de la transmisión del creador en SQLite.
*   **Cómo:** Filtrar el userlist del chat buscando usuarios con rol de moderador y guardar el conteo.

### 369. Alerta si la cuenta del viewer se queda sin tokens en el chat
*   **Por qué:** Evitar fallos al intentar interactuar o enviar propinas si el balance llega a cero.
*   **Cómo:** Monitorizar de saldo en el chat y mostrar avisos si baja de un límite configurado.

### 370. Guardado del historial de mensajes del chat en formato plano (.txt)
*   **Por qué:** Disponer de una bitácora de chat fácil de leer y buscar sin necesidad de reproductores de video.
*   **Cómo:** Escribir los mensajes del chat limpios de marcas de tiempo en un archivo de texto plano de logs.

### 371. Detección de enlaces compartidos en el chat
*   **Por qué:** Extraer y documentar enlaces web (ej. redes sociales) compartidos por el modelo en el chat.
*   **Cómo:** Analizar los textos de mensajes mediante expresiones regulares y guardar las URLs encontradas en SQLite.

### 372. Registro del color y tipografía preferida del broadcaster en el chat
*   **Por qué:** Documentar el estilo visual preferido del creador al escribir sus anuncios de chat.
*   **Cómo:** Extraer el color y tipografía de los mensajes marcados como provenientes del broadcaster.

### 373. Simulación de confirmación de lectura de mensajes privados
*   **Por qué:** Evitar alertar de uso de bot marcando mensajes como leídos de forma realista en el servidor de la plataforma.
*   **Cómo:** Enviar peticiones REST de confirmación de lectura simulando la interacción manual del usuario en DMs.

### 374. Alerta si el modelo escribe un mensaje destacado en el chat (Notice)
*   **Por qué:** Registrar anuncios importantes del modelo en la línea de tiempo de la grabación.
*   **Cómo:** Detectar mensajes del tipo "Notice" y guardarlos en una tabla de eventos destacados de la sesión.

### 375. Caching de las fotos de los perfiles de los usuarios del chat
*   **Por qué:** Mostrar fotos de perfil de usuarios destacados en las notificaciones del chat en la TUI.
*   **Cómo:** Descargar y cachear avatares de usuarios en una carpeta temporal `/tmp/cbrec_chat_avatars/`.

### 376. Detección de mensajes borrados por los moderadores en el chat
*   **Por qué:** Documentar la censura de mensajes específicos en la línea de tiempo del directo.
*   **Cómo:** Escuchar eventos de eliminación de mensajes en el socket y marcar el mensaje correspondiente en el SRT.

### 377. Filtro de chat por tokens del emisor
*   **Por qué:** Omitir mensajes de usuarios sin fichas y centrar los subtítulos en comentarios de usuarios con tokens.
*   **Cómo:** Validar si la propiedad `has_tokens` del emisor del mensaje es true en el payload JSON.

### 378. Notificación si un usuario específico del chat envía un mensaje
*   **Por qué:** Alertar al usuario si un amigo o donante conocido entra en la sala o escribe en el chat.
*   **Cómo:** Permitir configurar una lista de usuarios de interés y enviar notificaciones si escriben en la sala monitorizada.

### 379. Medidor de mensajes de chat por segundo (Chat Activity Rate)
*   **Por qué:** Evaluar la animación e interacción de la sala de forma numérica.
*   **Cómo:** Calcular la tasa de mensajes por segundo y guardarla en las series temporales de SQLite.

### 380. Detección automática del uso de emojis personalizados en el chat
*   **Por qué:** Documentar y guardar los emojis propietarios del creador compartidos en la sala.
*   **Cómo:** Parsear las URLs de imágenes de emojis incrustadas en el payload del mensaje del chat.

### 381. Exclusión opcional de mensajes del propio viewer en los subtítulos
*   **Por qué:** Evitar que los comentarios del propio usuario de la sesión empañen la visualización del video grabado.
*   **Cómo:** Omitir mensajes cuyo nombre de usuario coincida con el de la cuenta de cookies activa.

### 382. Registro de la permanencia de usuarios destacados en la sala
*   **Por qué:** Conocer cuánto tiempo pasan los mayores donantes del modelo viendo el stream.
*   **Cómo:** Rastrear eventos de entrada y salida del userlist del chat para calcular duraciones de visitas.

### 383. Caching de notas de usuarios locales para búsquedas rápidas
*   **Por qué:** Evitar llamadas redundantes a `/notes/usernames/` al interactuar con usuarios de la sala.
*   **Cómo:** Almacenar las notas de usuarios en una base SQLite local de rápido acceso.

### 384. Notificación si la mensajería privada está deshabilitada por el modelo
*   **Por qué:** Evitar intentos fallidos de enviar DMs al modelo si este ha cerrado su buzón de entrada.
*   **Cómo:** Comprobar las preferencias de mensajería del perfil del modelo antes de iniciar solicitudes de contacto.

### 385. Registro del tipo de fuente de los mensajes de chat
*   **Por qué:** Documentar las fuentes tipográficas de los mensajes para reproducirlas de forma fidedigna.
*   **Cómo:** Guardar el parámetro `font_family` de los mensajes recibidos en los logs de la sesión.

### 386. Detección de mensajes destacados de miembros del Fan Club (Club Notices)
*   **Por qué:** Identificar comentarios prioritarios de los miembros premium de la sala.
*   **Cómo:** Filtrar mensajes de chat que tengan la propiedad `in_fanclub` en true y guardarlos en SQLite.

### 387. Alerta si el modelo responde a un mensaje específico del viewer
*   **Por qué:** Notificar inmediatamente si el creador ha interactuado directamente con nosotros en el chat.
*   **Cómo:** Escuchar el chat y enviar una alerta de alta prioridad si el broadcaster menciona nuestro nombre de usuario.

### 388. Registro de mensajes con links a otras salas de Chaturbate
*   **Por qué:** Documentar referencias cruzadas de modelos recomendados en el chat del directo.
*   **Cómo:** Identificar URLs del tipo `chaturbate.com/{modelo}` en los textos de los mensajes de chat.

### 389. Caching de las opciones de renderizado de chat
*   **Por qué:** Evitar consultas redundantes de renderizado de chat durante ciclos continuos del daemon watch.
*   **Cómo:** Mantener en memoria el JSON de renderizado de chat por la duración de la ejecución del daemon.

### 390. Notificación si un usuario es silenciado (Muted User)
*   **Por qué:** Documentar la sanción temporal de usuarios conflictivos en la sala.
*   **Cómo:** Detectar eventos de silencio de chat y registrar el usuario y duración de la sanción en SQLite.

### 391. Filtro de chat por palabras clave de spam comunes
*   **Por qué:** Evitar guardar mensajes promocionales de bots en los subtítulos de la grabación.
*   **Cómo:** Definir un listado de palabras prohibidas y omitir mensajes coincidentes del archivo SRT.

### 392. Registro de estadísticas de mensajes enviados por tipo de usuario
*   **Por qué:** Analizar demográficamente qué tipos de usuarios (mods, fanclub, anónimos) son más activos en el chat.
*   **Cómo:** Contabilizar los mensajes agrupados por roles de usuario y guardar el resumen en SQLite.

### 393. Simulación de lectura de alertas de mensajería
*   **Por qué:** Mantener las notificaciones del perfil web limpias de alertas del daemon de grabación.
*   **Cómo:** Realizar llamadas de confirmación de alertas de notificaciones recibidas en segundo plano.

### 394. Notificación si el modelo cambia las reglas de chat (Room Rules)
*   **Por qué:** Informar al usuario si el creador ha reconfigurado los requisitos de interacción del chat.
*   **Cómo:** Capturar eventos del tipo `RoomSettingsTopic` en el WebSocket que detallen reglas del chat.

### 395. Registro de la densidad de mensajes del chat por minuto (Chat Density Log)
*   **Por qué:** Identificar momentos de gran excitación colectiva en la sala en base al volumen de comentarios.
*   **Cómo:** Registrar el conteo de mensajes de chat agrupados por minuto de grabación en SQLite.

### 396. Detección de Staff del sitio en la sala
*   **Por qué:** Registrar e informar si personal oficial de la plataforma (Staff) ha ingresado a la sala.
*   **Cómo:** Identificar usuarios del chat que tengan asignados roles específicos de personal de Chaturbate.

### 397. Exclusión de mensajes de chat en idiomas no deseados
*   **Por qué:** Mantener los subtítulos legibles excluyendo idiomas que el usuario no comprende.
*   **Cómo:** Aplicar filtros de detección de lenguaje ligeros en texto y omitir comentarios en idiomas desactivados.

### 398. Notificación si la sala pasa a modo exclusivo para miembros del Fan Club (Fan-Only Chat)
*   **Por qué:** Alertar si la sala restringe la conversación del chat a miembros de pago de forma temporal.
*   **Cómo:** Detectar cambios de privilegios del chat en el user_info y notificar al usuario.

### 399. Registro del histórico de mensajes de DMs enviados por modelo en SQLite
*   **Por qué:** Disponer de una bitácora de todos los mensajes privados enviados a cada creador de forma consolidada.
*   **Cómo:** Guardar copias de los DMs salientes de la cuenta en una tabla SQLite dedicada `outbound_dms`.

### 400. Caching de avatares del userlist para TUI
*   **Por qué:** Mostrar iconos de espectadores en la interfaz de terminal de forma fluida.
*   **Cómo:** Descargar y convertir avatares a caracteres de bloques Unicode de baja resolución para la TUI.

---

## 13. APIs REST - Seguimiento y Fan Club

### 401. Monitoreo del conteo de salas seguidas online en `/follow/api/online_followed_rooms/`
*   **Por qué:** Conocer el número total de modelos seguidos en vivo de forma resumida para paneles gráficos.
*   **Cómo:** Consultar el campo `online` de la respuesta JSON para actualizar contadores en la TUI.

### 402. Extracción de imágenes de modelos seguidos en la lista online
*   **Por qué:** Disponer de los avatares correspondientes de las salas en watch de forma rápida.
*   **Cómo:** Mapear la URL de la clave `image` dentro de `online_rooms` y descargar los avatares correspondientes.

### 403. Automatización de seguimiento con `/follow/follow/{username}/`
*   **Por qué:** Permitir al usuario seguir a un modelo de interés directamente desde el reproductor de la CLI sin abrir la web.
*   **Cómo:** Hacer una petición POST al endpoint enviando el token CSRF y el User-Agent correspondiente en las cabeceras.

### 404. Automatización de dejar de seguir con `/follow/unfollow/{username}/`
*   **Por qué:** Desmarcar modelos de la lista de seguimiento del usuario directamente desde la terminal.
*   **Cómo:** Realizar un POST con CSRF y cookies de sesión activas para remover al creador de la lista.

### 405. Detección de frecuencia de notificaciones preferida de los seguidos
*   **Por qué:** Ajustar la prioridad de monitoreo según la configuración de notificaciones del usuario (smart/always/never).
*   **Cómo:** Validar el campo `notification_frequency` retornado tras realizar la acción de seguimiento.

### 406. Caching del estado de seguimiento de los modelos en SQLite
*   **Por qué:** Evitar llamadas HTTP innecesarias para comprobar si ya seguimos a un creador en cada inicio del watch.
*   **Cómo:** Guardar una flag booleana `is_following` en la tabla SQLite de modelos con una validez de 24 horas.

### 407. Notificación si un modelo seguido inicia show con frecuencia inteligente ("smart")
*   **Por qué:** Enviar notificaciones de escritorio prioritarias que coincidan con el criterio del sitio web.
*   **Cómo:** Leer el campo de frecuencia de notificaciones al iniciar watch y priorizar los avisos visuales.

### 408. Búsqueda automatizada en lote de salas de fan club disponibles con `/fanclub/join/{username}/`
*   **Por qué:** Validar los costes de ingreso al club de fans de múltiples modelos monitorizados de forma automatizada.
*   **Cómo:** Realizar peticiones GET al endpoint de join y parsear el HTML resultante buscando tarifas y ventajas.

### 409. Procesamiento automático de suscripciones al Fan Club via `/fanclub/process/{username}/`
*   **Por qué:** Renovar suscripciones a creadores de alta prioridad antes de que expiren y perdamos acceso de grabación.
*   **Cómo:** Realizar un POST al endpoint enviando parámetros de meses, coste y tokens requeridos junto al token CSRF.

### 410. Simulación de suscripción con cargo en dólares (`subscription_signup=1`)
*   **Por qué:** Permitir configurar compras de membresía de fan club a través de tarjeta de crédito/débito desde el daemon.
*   **Cómo:** Pasar el parámetro `subscription_signup=1` y el coste en centavos de dólar en la petición de compra.

### 411. Simulación de suscripción con tokens (`token_signup=1`)
*   **Por qué:** Adquirir acceso al fan club usando el saldo de tokens de la cuenta.
*   **Cómo:** Pasar los parámetros `token_signup=1` y la cantidad de tokens requerida al endpoint de procesamiento.

### 412. Detección automática del coste mensual del Fan Club en tokens
*   **Por qué:** Conocer el coste en tokens para avisar si el saldo es insuficiente antes de intentar suscribirse.
*   **Cómo:** Analizar la respuesta del endpoint de join para extraer el valor numérico de tokens mensuales.

### 413. Notificación de fin de suscripción al Fan Club
*   **Por qué:** Prevenir que se interrumpa la grabación de streams exclusivos por pérdida de suscripción activa.
*   **Cómo:** Calcular la fecha de vencimiento en base a la fecha de procesamiento SQLite y alertar 3 días antes.

### 414. Caching dinámico del listado de salas seguidas
*   **Por qué:** Optimizar el rendimiento de red reduciendo consultas a `/online_followed_rooms/`.
*   **Cómo:** Almacenar el JSON retornado en memoria RAM con un tiempo de expiración corto de 2 minutos.

### 415. Detección de errores de baneo de cuenta de seguidor
*   **Por qué:** Evitar loops si la cuenta de cookies de seguidor es baneada por Chaturbate por actividad sospechosa.
*   **Cómo:** Detectar si el endpoint de seguidos devuelve redirecciones o errores 403 e informar al usuario en consola.

### 416. Filtrado de salas seguidas online por género en la TUI
*   **Por qué:** Organizar de forma rápida los directos de interés en paneles interactivos independientes.
*   **Cómo:** Combinar la respuesta de salas seguidas con la consulta del roomlist para filtrar categorías.

### 417. Auto-limpieza de la lista de seguidos inactivos
*   **Por qué:** Ayudar a mantener limpia la cuenta eliminando modelos que no transmiten hace meses.
*   **Cómo:** Si un modelo no ha transmitido por 90 días, ofrecer la opción CLI de mandar un unfollow automático.

### 418. Detección de cambios de tarifa del Fan Club
*   **Por qué:** Rastrear si los modelos de interés incrementan o decrementan el coste de su club de fans.
*   **Cómo:** Consultar el endpoint de join periódicamente y registrar variaciones de precios en SQLite.

### 419. Registro de la fecha exacta de suscripción al club de fans en SQLite
*   **Por qué:** Disponer de una bitácora financiera de las compras realizadas en la plataforma.
*   **Cómo:** Guardar los parámetros de fecha, modelo y coste tras recibir respuesta exitosa del POST `/process/`.

### 420. Detección automática de promociones del Fan Club (Tarifas con descuento)
*   **Por qué:** Informar al usuario si los creadores monitorizados ofrecen ofertas temporales de ingreso.
*   **Cómo:** Analizar las respuestas del HTML de join buscando etiquetas indicativas de descuento o meses gratuitos.

### 421. Soporte para simular orígenes de join (SupporterSourceJoinFanClubButton)
*   **Por qué:** Reducir detección de bots enviando el parámetro de tracking exacto que usa la web original.
*   **Cómo:** Añadir `?source=SupporterSourceJoinFanClubButton` a las URIs de join del cliente HTTP.

### 422. Caching de cookies específicas del subdominio de seguidos
*   **Por qué:** Evitar redirecciones innecesarias entre subdominios al gestionar el listado de seguidos.
*   **Cómo:** Forzar el jar de cookies a retener sesiones específicas del subdominio de follow.

### 423. Notificación si la cuenta no cuenta con saldo para Fan Club
*   **Por qué:** Alertar al usuario si una compra automatizada fallará por falta de saldo en tokens.
*   **Cómo:** Comparar el precio del club en el JSON de join con el saldo de tokens de `/tipping/current_tokens/`.

### 424. Registro de salas seguidas online ausentes de los listados públicos
*   **Por qué:** Identificar si un modelo seguido transmite en modo oculto ("hidden") solo para sus followers.
*   **Cómo:** Comparar los modelos en `/online_followed_rooms/` con las respuestas del roomlist público.

### 425. Exportación del listado de modelos seguidos en formato JSON
*   **Por qué:** Facilitar copias de seguridad de la lista de seguidos para migración de cuentas.
*   **Cómo:** Subcomando `cbrec follow export` que consulte la API y guarde el listado estructurado a disco.

### 426. Importación masiva de lista de modelos seguidos
*   **Por qué:** Facilitar el seguimiento de múltiples creadores al configurar una cuenta nueva.
*   **Cómo:** Subcomando `cbrec follow import {archivo.json}` que realice peticiones POST consecutivas a `/follow/`.

### 427. Alerta si un modelo de alta prioridad elimina su Fan Club
*   **Por qué:** Informar si las ventajas de grabación exclusiva de esa sala ya no están disponibles en el sitio.
*   **Cómo:** Detectar si el endpoint de join devuelve errores de redirección a páginas de perfil genéricas.

### 428. Sincronización automática de modelos en watch a partir de la lista de seguidos
*   **Por qué:** Olvidarse de editar archivos TOML y monitorizar dinámicamente todo lo que seguimos en la web.
*   **Cómo:** Configurar la flag `cbrec watch --sync-followed` para actualizar la lista de monitorización en cada ciclo.

### 429. Detección de baneo del endpoint de seguidos por Cloudflare
*   **Por qué:** Evitar la exposición de la IP si Cloudflare detecta scraping en el área privada de seguidos.
*   **Cómo:** Comprobar si las llamadas retornan código 403 con firma de Cloudflare y frenar las peticiones de inmediato.

### 430. Registro histórico del total de seguidos online del servidor
*   **Por qué:** Graficar en qué horarios y días de la semana hay más creadores favoritos transmitiendo a la vez.
*   **Cómo:** Almacenar el conteo numérico de `/online_followed_rooms/` en una tabla SQLite de telemetría horaria.

### 431. Simulación de reintentos asíncronos en fallas de unfollow
*   **Por qué:** Asegurar que las acciones de desmarcado de modelos se completen ante fallas de red.
*   **Cómo:** Encolar peticiones de unfollow en SQLite y reintentar su ejecución asíncronamente si falla la red.

### 432. Notificación en la TUI del tiempo restante de suscripción al club
*   **Por qué:** Mostrar visualmente el estado del acceso premium al lado del nombre de cada creador.
*   **Cómo:** Dibujar un indicador temporal (ej. "Club: 12d restantes") en las filas correspondientes de la TUI.

### 433. Caching de avatares del listado de seguidos
*   **Por qué:** Ahorrar ancho de banda guardando imágenes estáticas de los perfiles de los seguidos.
*   **Cómo:** Almacenar miniaturas en disco con nombres basados en el hash MD5 de sus URLs de origen.

### 434. Detección automática del número de meses suscrito al club de fans
*   **Por qué:** Mostrar en los reportes HTML la antigüedad del usuario como miembro de soporte del creador.
*   **Cómo:** Leer el contador de meses de suscripción del DOM del HTML de join del modelo.

### 435. Simulación de peticiones cruzadas para validar cookies de seguidos
*   **Por qué:** Asegurar que las cookies de sesión son capaces de operar en subdominios de seguidos de forma robusta.
*   **Cómo:** Enviar un GET de prueba a `/follow/api/online_followed_rooms/` antes de iniciar el watch principal.

### 436. Filtrado interactivo en la CLI de seguidos no grabados
*   **Por qué:** Identificar rápidamente a creadores favoritos que están transmitiendo pero que no estamos grabando.
*   **Cómo:** Comparar la lista de directos seguidos con los hilos activos de FFmpeg en la consola de comandos.

### 437. Registro de comentarios e interacciones de DMs en salas de Fan Club
*   **Por qué:** Archivar interacciones en el club de fans para auditorías del usuario.
*   **Cómo:** Guardar logs de mensajes de DMs específicos de creadores en watch en SQLite.

### 438. Notificación si un modelo que no seguimos nos añade a su club
*   **Por qué:** Detectar si hemos recibido acceso promocional o de regalo al club de fans de un modelo.
*   **Cómo:** Monitorear notificaciones de bell de tipo club y enviar avisos de alta prioridad.

### 439. Simulación de cabecera Sec-Fetch-User en peticiones de join
*   **Por qué:** Cumplir con las firmas de seguridad de cabeceras de navegación manual del usuario.
*   **Cómo:** Añadir `Sec-Fetch-User: ?1` al realizar el GET para obtener la página de join del club.

### 440. Detección de baneo silencioso de DMs del Fan Club
*   **Por qué:** Advertir si el buzón de entrada no procesa los mensajes del club de fans.
*   **Cómo:** Si las peticiones de envío retornan éxito pero no figuran en las respuestas de pm_list del modelo.

### 441. Caching de listados de fan club en SQLite para búsquedas offline
*   **Por qué:** Buscar información sobre tarifas de clubs guardados sin conexión de red.
*   **Cómo:** Almacenar datos históricos de join mapeados a fechas en tablas SQLite locales.

### 442. Alerta si las cookies carecen de sesión para seguir modelos
*   **Por qué:** Evitar llamadas fallidas a `/follow/` indicando temprano que la acción requiere login completo.
*   **Cómo:** Verificar la existencia de cookies de autenticación críticas en el jar de red antes de ejecutar el POST.

### 443. Simulación de reintentos inteligentes ante fallas de cookies en DMs del club
*   **Por qué:** Renovar la sesión de cookies antes de reintentar el envío de mensajes privados del club.
*   **Cómo:** Lanzar tareas de regeneración de cookies si el pm_list del club devuelve 401 de forma continua.

### 444. Registro de estadísticas de suscripciones mensuales
*   **Por qué:** Evaluar gastos y suscripciones activas del grabador a lo largo del año.
*   **Cómo:** Agrupar las compras registradas en SQLite por meses y generar reportes financieros consolidados.

### 445. Notificación si el club de fans cambia sus ventajas descritas
*   **Por qué:** Informar si las reglas o promesas de contenido del creador han cambiado en el sitio.
*   **Cómo:** Guardar hash del texto de ventajas en SQLite y alertar si la consulta de join devuelve cambios textuales.

### 446. Simulación de click de consentimiento en compras del club
*   **Por qué:** Evitar fallas de autenticación enviando los checks de términos de uso requeridos en el POST.
*   **Cómo:** Añadir variables de confirmación de términos a los payloads del POST de suscripción.

### 447. Caching de avatares del listado de seguidos en TUI Unicode
*   **Por qué:** Visualizar avatares en la interfaz de consola sin dependencias pesadas de librerías.
*   **Cómo:** Procesar e indexar avatares ligeros en formato ANSI para dibujo en el panel de TUI.

### 448. Detección automática de la fecha de caducidad exacta del acceso al club
*   **Por qué:** Mostrar alertas de expiración precisas y sincronizar renovaciones de cookies al segundo.
*   **Cómo:** Parsear las fechas de expiración textuales devueltas en el perfil del club de fans.

### 449. Simulación de peticiones OPTIONS dinámicas a endpoints de seguimiento
*   **Por qué:** Evitar bloqueos WAF enviando preflights lógicos antes de seguir o dejar de seguir creadores.
*   **Cómo:** Realizar peticiones OPTIONS cortas previas a las URIs `/follow/` en el módulo de red.

### 450. Notificación si un modelo seguido cambia a show exclusivo de Fan Club
*   **Por qué:** Iniciar el grabador con privilegios de cookies correspondientes al club de fans al instante.
*   **Cómo:** Detectar el cambio de estado de sala a fan-only y conmutar el hilo de descarga al playlist autorizado.

---

## 14. APIs REST - Propinas y Finanzas

### 451. Automatización de envío de propinas mediante `/tipping/send_tip/{username}/`
*   **Por qué:** Enviar propinas de forma automatizada al chat en base a eventos de grabación o comandos CLI.
*   **Cómo:** Realizar un POST con token CSRF, monto, tipo de show, firma de seguridad y mensaje de propina.

### 452. Consulta automática de tokens en la cuenta con `/tipping/current_tokens/?room={room}`
*   **Por qué:** Monitorear el saldo disponible en tokens en la cuenta de cookies de forma desasistida.
*   **Cómo:** Consultar el endpoint enviando el parámetro de la sala y guardar el balance de tokens en SQLite.

### 453. Consulta del modelo de tarifas de propinas con `/tipping/rate_model/{username}/`
*   **Por qué:** Conocer la tasa de propinas configurada por el modelo para decidir montos óptimos de envío.
*   **Cómo:** Hacer un GET al endpoint de rate model y registrar los límites y tarifas en los metadatos.

### 454. Envío de comentarios asociados a propinas en `/tipping/add_comment/{username}/`
*   **Por qué:** Añadir mensajes personalizados de agradecimiento o sugerencias al chat junto al envío de propinas.
*   **Cómo:** Realizar POST con la cookie de sesión y el texto del comentario tras ejecutar la propina.

### 455. Consulta de propinas enviadas en las últimas 24h en `/tipping/tips_in_last_24/`
*   **Por qué:** Rastrear si la cuenta del grabador ya ha colaborado con el modelo en el último día.
*   **Cómo:** Consultar el endpoint y registrar el estado en los logs de la sesión de grabación.

### 456. Monitoreo de facturas criptográficas con `/tipping/crypto_invoices/`
*   **Por qué:** Facilitar el pago o recarga de tokens usando criptomonedas directamente desde el grabador.
*   **Cómo:** Consultar y generar facturas de recarga de tokens usando la API de facturas cripto de la cuenta.

### 457. Consulta de membresías de tokens en `/api/ts/tipping/memberships/`
*   **Por qué:** Ofrecer información de compra de planes de tokens en el dashboard del grabador.
*   **Cómo:** Hacer un GET al endpoint de memberships y presentar los planes de tokens disponibles en la TUI.

### 458. Consulta de estadísticas de consumo de tokens en `/api/ts/tipping/token-stats/`
*   **Por qué:** Graficar el gasto de tokens a lo largo del tiempo de forma detallada.
*   **Cómo:** Consultar las estadísticas de tokens y guardarlas en SQLite para análisis e históricos.

### 459. Consulta del historial de valoraciones en `/api/ts/tipping/rating-history/`
*   **Por qué:** Conocer las calificaciones otorgadas a modelos tras el envío de propinas históricas.
*   **Cómo:** Obtener y guardar el historial de valoraciones en la base de datos local del grabador.

### 460. Descarga del historial completo de propinas en CSV mediante `/tipping/csv/history/`
*   **Por qué:** Importar y consolidar todos los gastos de propinas de la cuenta en hojas de cálculo externas.
*   **Cómo:** Descargar el archivo CSV de historial de propinas de la cuenta y archivarlo de forma automatizada.

### 461. Simulación de envío de propina oculta o anónima
*   **Por qué:** Proteger la identidad de la cuenta del grabador en directos grabados públicamente.
*   **Cómo:** Enviar la propina configurando `tip_type: "anonymous"` y `anonymous: true` en el POST.

### 462. Simulación de envío de ítems del menú de propinas (Tip Menu)
*   **Por qué:** Solicitar acciones o shows interactivas automáticas enviando tokens vinculados a ítems del menú.
*   **Cómo:** POST al send_tip agregando `menu_id`, `item_id` y `source: "tip_menu"` en el JSON de la petición.

### 463. Caching de saldo de tokens en la barra de la TUI
*   **Por qué:** Mostrar el saldo actual del usuario en la pantalla de la TUI de forma constante sin recargar red.
*   **Cómo:** Mantener una variable global de balance y actualizarla únicamente tras eventos de envío o recarga.

### 464. Alerta si el saldo de tokens baja de un mínimo crítico
*   **Por qué:** Advertir temprano si no hay saldo suficiente para enviar propinas de agradecimiento o renovar clubs.
*   **Cómo:** Chequear el saldo y enviar notificaciones móviles si cae por debajo de 50 tokens.

### 465. Registro del total de tokens gastados por modelo en SQLite
*   **Por qué:** Disponer de estadísticas del gasto realizado por cada creador monitorizado de forma agregada.
*   **Cómo:** Sumar y registrar el costo de cada propina enviada con éxito en la tabla `model_expenses`.

### 466. Detección automática del saldo en dólares de tokens en cashout (`/tipping/cashout_tokens/`)
*   **Por qué:** Rastrear el saldo pendiente de cobro en caso de usar el grabador con cuentas de broadcasters.
*   **Cómo:** Consultar el endpoint de cashout y guardar los valores financieros en las tablas de SQLite.

### 467. Consulta de estado de concurso de salas con `/contest/log/{username}/`
*   **Por qué:** Conocer si la sala está participando en concursos o eventos especiales de tokens en el sitio.
*   **Cómo:** Hacer GET al log de concurso y documentar el avance del modelo en las estadísticas de la sesión.

### 468. Simulación del origen de propina desde menú personalizado
*   **Por qué:** Cumplir con las firmas de telemetría de propinas de la web para evitar sospechas de bots.
*   **Cómo:** Pasar el parámetro `source: "tip_menu_custom"` en propinas de montos manuales en la API.

### 469. Validación de firmas de seguridad de propinas
*   **Por qué:** Evitar rechazos de transacciones por tokens de firma inválidos en el POST de propinas.
*   **Cómo:** Extraer el token de firma `sig` del DOM de la sala antes de iniciar la solicitud de envío.

### 470. Caching dinámico del menú de propinas del modelo
*   **Por qué:** Conocer los ítems de propinas válidos del creador sin consultar la web continuamente.
*   **Cómo:** Parsear el canal `RoomTipMenuTopic` en WebSocket y guardar el menú en memoria.

### 471. Alerta si un modelo de alta prioridad cambia los precios de su menú de propinas
*   **Por qué:** Informar si las acciones interactivas del creador han cambiado de precio en tokens.
*   **Cómo:** Comparar el menú recibido en WebSocket con el guardado en base de datos y notificar variaciones.

### 472. Registro del histórico de valoraciones del modelo en la base SQLite
*   **Por qué:** Registrar las valoraciones del creador asociadas a grabaciones de la sesión en disco.
*   **Cómo:** Escribir en base de datos el valor de la calificación otorgada tras recibir confirmación.

### 473. Simulación de reintentos asíncronos en fallas de envío de propinas
*   **Por qué:** Asegurar que las donaciones automáticas se completen ante fallas temporales de red.
*   **Cómo:** Encolar la propina en SQLite y reintentar su envío cuando la conexión de red se restablezca.

### 474. Notificación visual si el balance de tokens es actualizado en la cuenta
*   **Por qué:** Informar al usuario en consola si una compra o recarga de tokens ha sido procesada con éxito.
*   **Cómo:** Detectar cambios al alza en `/current_tokens/` y mostrar un banner destacado de recarga.

### 475. Registro de estadísticas de gasto por mes en la base SQLite
*   **Por qué:** Disponer de un resumen financiero del grabador a lo largo del año para control de gastos.
*   **Cómo:** Agrupar las propinas de base de datos SQLite por meses y generar reportes financieros consolidados.

### 476. Detección automática del tipo de show para envío de propinas
*   **Por qué:** Asegurar que el envío de propinas se categorice correctamente según el show actual (public/private).
*   **Cómo:** Leer el estado de la sala en `RoomStatusTopic` y pasar la flag correspondiente en el POST.

### 477. Caching de facturas criptográficas locales para reintentos de pago
*   **Por qué:** Evitar generar múltiples facturas de pago cripto redundantes si la red falla momentáneamente.
*   **Cómo:** Guardar la factura criptográfica generada en disco con un TTL de 1 hora.

### 478. Alerta si la firma de seguridad de propinas ha caducado
*   **Por qué:** Renovar el token de seguridad antes de intentar enviar propinas para evitar fallas 400.
*   **Cómo:** Comprobar la validez del token de firma y solicitar uno nuevo a la API de la sala si es necesario.

### 479. Simulación de origen de propina desde la barra de chat
*   **Por qué:** Enviar propinas imitando el flujo rápido de la barra de chat de la web original.
*   **Cómo:** Pasar los parámetros correspondientes de chat source en los cuerpos POST de la transacción.

### 480. Registro del ID único de transacción de propinas en SQLite
*   **Por qué:** Disponer del ID único del sitio para reclamos o validaciones cruzadas de propinas.
*   **Cómo:** Extraer el ID de transacción de la respuesta exitosa del POST de send_tip.

### 481. Notificación si la cuenta no cuenta con saldo para propinas automáticas
*   **Por qué:** Alertar al usuario si una donación automática fallará por falta de saldo en tokens.
*   **Cómo:** Comparar el costo de la propina con el saldo de tokens de `/tipping/current_tokens/`.

### 482. Simulación de envío de propina silenciosa con confirmación de sonido desactivada
*   **Por qué:** Evitar ruidos o avisos del sistema local al enviar propinas automáticas.
*   **Cómo:** Pasar la flag correspondiente en los parámetros JSON de la petición HTTP.

### 483. Caching de planes de membresías locales para consultas offline
*   **Por qué:** Consultar planes de tokens guardados sin requerir peticiones de red activas.
*   **Cómo:** Almacenar planes del all-memberships en SQLite con un tiempo de vida largo.

### 484. Detección automática de promociones de compra de tokens
*   **Por qué:** Informar al usuario si la plataforma ofrece descuentos en la compra de paquetes de tokens.
*   **Cómo:** Buscar patrones de descuento en los strings devueltos en el endpoint de memberships.

### 485. Registro del saldo total en tokens gastados de forma global en SQLite
*   **Por qué:** Conocer el gasto histórico acumulado del grabador en la plataforma.
*   **Cómo:** Guardar y actualizar un contador del total de tokens consumidos en la base SQLite.

### 486. Simulación de envío de propina con mensaje largo de chat
*   **Por qué:** Compartir mensajes de gran longitud asociados a propinas en la sala.
*   **Cómo:** Pasar el texto largo en la clave `message` del cuerpo JSON del POST.

### 487. Notificación si la tasa de propinas cambia drásticamente en el rate_model
*   **Por qué:** Informar si el creador ha reconfigurado los rangos de propinas recomendados.
*   **Cómo:** Monitorear cambios en los valores de retorno de rate_model y guardar la alerta.

### 488. Registro de estadísticas de valoraciones mensuales del grabador
*   **Por qué:** Conocer el histórico de calificaciones otorgadas por el usuario a lo largo de los meses.
*   **Cómo:** Agrupar las valoraciones SQLite por fecha de registro y generar reportes consolidados.

### 489. Simulación de envío de propina en shows privados exclusivos
*   **Por qué:** Enviar propinas en shows de pago privados respetando el contexto de visualización restringido.
*   **Cómo:** Pasar `tip_room_type: "private"` en el cuerpo JSON de la transacción.

### 490. Detección automática de cobros de propinas fallidos por límites de cuenta
*   **Por qué:** Advertir si la cuenta del usuario tiene restricciones de transacciones en tokens.
*   **Cómo:** Analizar respuestas de error de send_tip del tipo "account limit reached" y alertar.

### 491. Caching de estadísticas de consumo de tokens locales
*   **Por qué:** Optimizar la velocidad del dashboard web de estadísticas locales.
*   **Cómo:** Guardar los resultados agregados de token-stats en la base de datos de SQLite.

### 492. Alerta si las cookies carecen de tokens para propinas
*   **Por qué:** Evitar llamadas fallidas a `/send_tip/` indicando temprano que la acción requiere saldo de tokens.
*   **Cómo:** Verificar que el balance retornado por current_tokens sea mayor a cero antes de proceder.

### 493. Simulación de reintentos inteligentes ante fallas de cookies en transacciones
*   **Por qué:** Evitar rechazos de transacciones renovando la sesión de cookies antes de reintentar.
*   **Cómo:** Lanzar tareas de inicio de sesión automáticas si el send_tip devuelve 401 de forma continua.

### 494. Registro de estadísticas de propinas enviadas por tipo de show en SQLite
*   **Por qué:** Analizar en qué tipos de shows (públicos, privados, spy) gasta más el grabador.
*   **Cómo:** Almacenar la propiedad de tipo de show asociada a cada propina y agruparlas en reportes.

### 495. Notificación si el modelo cambia el precio mínimo del tip menu
*   **Por qué:** Informar si las opciones de propinas más económicas del creador han cambiado.
*   **Cómo:** Analizar variaciones en el ítem de menor valor del menú de propinas en WebSocket.

### 496. Simulación de envío de propina desde la aplicación móvil simulada
*   **Por qué:** Utilizar flujos de transacciones simplificados propios de la versión móvil del sitio.
*   **Cómo:** Enviar la petición simulando el User-Agent móvil y cabeceras simplificadas de red.

### 497. Caching de valoraciones en SQLite locales para análisis de satisfacción
*   **Por qué:** Disponer de información agregada de modelos mejor calificados por el usuario.
*   **Cómo:** Guardar las valoraciones SQLite asociadas a los nombres de modelos en la base local.

### 498. Alerta si el modelo ha deshabilitado el envío de comentarios en propinas
*   **Por qué:** Evitar enviar textos asociados si el modelo restringe las propinas a envíos mudos.
*   **Cómo:** Comprobar las preferencias devueltas en el rate_model antes de procesar el comentario.

### 499. Registro del histórico de saldo de tokens del grabador en SQLite
*   **Por qué:** Graficar la recarga y consumo de tokens a lo largo de las semanas de forma visual.
*   **Cómo:** Guardar el saldo actual en base de datos cada 24 horas en tareas programadas de fondo.

### 500. Simulación de clicks de confirmación de propinas de gran volumen
*   **Por qué:** Evitar rechazos de seguridad del sitio al enviar cantidades inusualmente grandes de tokens.
*   **Cómo:** Añadir variables de bypass de seguridad de doble confirmación de tokens en la petición.

---

## 15. WebSocket - Autenticación y Conectividad Ably

### 501. Obtención automatizada de token asíncrono en `/push_service/auth/`
*   **Por qué:** La conexión WebSocket de Ably expira y requiere un JWT firmado periódicamente.
*   **Cómo:** POST a `/push_service/auth/` con cookies de sesión y CSRF enviando la lista de canales deseados.

### 502. Envío dinámico de lista de canales en el POST de auth
*   **Por qué:** Suscribirse solo a canales de modelos de interés para evitar sobrecargas de red.
*   **Cómo:** Codificar la lista de canales en formato JSON Map en el parámetro `topics` del POST.

### 503. Simulación de backend para autenticación de Ably
*   **Por qué:** Enviar el backend exacto esperado por el servidor de push de Chaturbate.
*   **Cómo:** Pasar el parámetro `backend=a` en el cuerpo del POST de solicitud de autenticación de Ably.

### 504. Detección automática del host de WebSocket en la respuesta de auth
*   **Por qué:** Conectarse al balanceador de carga de WebSocket correcto indicado por la API.
*   **Cómo:** Leer e inicializar el socket usando el valor de la clave `host` de la respuesta de auth.

### 505. Extracción de canales asignados dinámicamente en la respuesta de auth
*   **Por qué:** Conectar el socket a las salas de Ably mapeadas para cada modelo por el backend.
*   **Cómo:** Mapear el mapa de strings de la clave `channels` del JSON retornado de auth de Ably.

### 506. Manejo automático del token JWT de Ably en las cabeceras del socket
*   **Por qué:** Completar la autenticación inicial de la conexión TCP del WebSocket de forma exitosa.
*   **Cómo:** Enviar la cabecera `X-Ably-Auth` con el token JWT de la respuesta de auth de Ably en el handshake.

### 507. Simulación del cliente de presencia `client_id` en Ably
*   **Por qué:** Evitar desconexiones de la red de Ably usando identificadores coherentes con la sesión.
*   **Cómo:** Configurar el identificador del socket como el valor de `client_id` devuelto por la API de auth.

### 508. Obtención automatizada del historial de canales en `/push_service/room_history/`
*   **Por qué:** Cargar mensajes de chat y alertas previas al inicio de la conexión activa del socket.
*   **Cómo:** Realizar POST a `/push_service/room_history/` enviando la lista de topics y cookies asociadas.

### 509. Suscripción dinámica a canales asíncronos en vivo (Ably Channels)
*   **Por qué:** Añadir o quitar suscripciones de modelos del socket activo al vuelo sin reiniciar.
*   **Cómo:** Enviar tramas de suscripción de Ably (`ATTACH` / `DETACH`) a través del canal de control del socket.

### 510. Reconexión Exponential Backoff asíncrona de Ably
*   **Por qué:** Garantizar estabilidad del socket ante cortes de red sin colapsar el servidor de Chaturbate.
*   **Cómo:** Reintentar conexiones de red duplicando retardos progresivamente si falla Ably.

### 511. Gestión de fallas de autenticación de Ably en la respuesta de auth
*   **Por qué:** Identificar de forma temprana si las peticiones de canales no fueron autorizadas.
*   **Cómo:** Validar el objeto `failures` de la respuesta de auth y alertar de canales rechazados.

### 512. Rotación dinámica de servidores de Ably (Fallbacks de red)
*   **Por qué:** Evitar la pérdida de monitoreo si la IP principal de Ably es bloqueada.
*   **Cómo:** Intentar secuencialmente conectar a hosts de fallback provistos si el host principal no responde.

### 513. Simulación de tramas PING de Ably en el socket
*   **Por qué:** Mantener la conexión activa evitando timeouts de inactividad de los balanceadores.
*   **Cómo:** Enviar tramas JSON estructuradas de PING de Ably de forma periódica en el WebSocket.

### 514. Gestión de tramas PONG de Ably en el socket
*   **Por qué:** Responder correctamente a los pings enviados por los servidores de Ably de forma realista.
*   **Cómo:** Responder con tramas de PONG válidas a los PINGs del servidor de forma asíncrona.

### 515. Detección automática de desconexiones forzadas por el servidor (Ably Close Event)
*   **Por qué:** Reiniciar el flujo de autenticación si el servidor de Ably cierra el socket de golpe.
*   **Cómo:** Capturar tramas de cierre del socket y disparar tareas de renovación de JWT de Ably.

### 516. Registro de logs de conexión del socket en SQLite
*   **Por qué:** Diagnosticar la estabilidad de la conexión de red del grabador a lo largo del tiempo.
*   **Cómo:** Guardar marcas de inicio y fin de conexión y errores de Ably en tablas de logs de red.

### 517. Caching del token JWT en memoria RAM con expiración lógica
*   **Por qué:** Evitar peticiones redundantes de autenticación REST si el socket se reconecta rápido.
*   **Cómo:** Almacenar el JWT con un tiempo de validez de 1 hora y reutilizarlo si no ha expirado.

### 518. Simulación del cliente de presencia móvil en Ably
*   **Por qué:** Reducir la detección de bots simulando la conexión de la app móvil en el socket.
*   **Cómo:** Usar cabeceras y estructuras de identificación de presencia móvil en el handshake.

### 519. Envío dinámico de variables de telemetría de red al conectar
*   **Por qué:** Evitar bloqueos de red enviando telemetría realista en la conexión del socket de Ably.
*   **Cómo:** Pasar parámetros de versión de cliente y estado de red en los query params de conexión.

### 520. Gestión de errores de red TCP en la conexión de Ably
*   **Por qué:** Saber si las fallas son de DNS, red física local o baneo de IP de Ably.
*   **Cómo:** Capturar errores de socket de bajo nivel y mapearlos a variantes descriptivas.

### 521. Caching del host de Ably activo en la base SQLite
*   **Por qué:** Disponer de históricos para evaluar qué servidores de Ably ofrecen mejor estabilidad.
*   **Cómo:** Guardar la IP y host de la conexión activa en tablas de logs de red.

### 522. Alerta si el token de Ably ha expirado en la conexión activa
*   **Por qué:** Renovar el token antes de que sea rechazado para evitar interrupciones de monitoreo.
*   **Cómo:** Comprobar la validez temporal del JWT de Ably de forma activa en el daemon watch.

### 523. Simulación de reintentos inteligentes ante fallas de cookies en auth
*   **Por qué:** Evitar loops de error si el POST de auth falla por cookies caducadas.
*   **Cómo:** Iniciar tareas de refresco de cookies si `/push_service/auth/` devuelve códigos 401.

### 524. Registro de estadísticas de latencia de mensajes de Ably en SQLite
*   **Por qué:** Conocer el lag de los servidores de Chaturbate en el envío de eventos de chat.
*   **Cómo:** Comparar el timestamp del mensaje recibido con la hora local y guardar la diferencia.

### 525. Notificación si la conexión de Ably entra en modo suspendido (Suspended State)
*   **Por qué:** Advertir si la conexión no se ha recuperado tras intentos de reconexión sucesivos.
*   **Cómo:** Detectar el estado suspendido en los eventos del cliente y enviar notificaciones móviles.

### 526. Simulación de origen de conexión de Ably desde subdominio de chat
*   **Por qué:** Enviar cabeceras de origen realistas en la conexión inicial del socket de Ably.
*   **Cómo:** Configurar la cabecera `Origin` como `https://chaturbate.com` en el cliente WebSocket.

### 527. Registro del ID único de cliente de presencia de Ably en SQLite
*   **Por qué:** Disponer del ID único para reclamos o validaciones cruzadas de eventos de red.
*   **Cómo:** Extraer el `client_id` de la respuesta de auth y guardarlo en tablas de logs.

### 528. Notificación si la conexión de Ably se realiza contra un host de fallback
*   **Por qué:** Saber si el host principal está caído y evaluar la calidad de la red doméstica.
*   **Cómo:** Comprobar si el host de conexión contiene la cadena de fallback y mostrar un aviso.

### 529. Simulación de envío de tramas de estado de actividad en el socket
*   **Por qué:** Mantener la visibilidad de la sesión en el panel de control web original.
*   **Cómo:** Enviar tramas de actualización de presencia a intervalos en el WebSocket de Ably.

### 530. Caching de canales de Ably locales para consultas offline
*   **Por qué:** Consultar los canales asignados a modelos guardados sin red activa.
*   **Cómo:** Almacenar el mapa de canales en la base SQLite local de rápido acceso.

### 531. Detección automática de la IP del servidor de Ably conectado
*   **Por qué:** Conocer a qué servidor físico nos estamos conectando para análisis de red.
*   **Cómo:** Resolver el hostname del host de Ably y registrar la IP final en las tablas SQLite.

### 532. Alerta si el host de Ably devuelto en auth difiere de la base
*   **Por qué:** Detectar si Chaturbate ha migrado o añadido nuevos servidores de push en la nube.
*   **Cómo:** Comparar el host recibido con los registrados y guardar la alerta si es nuevo.

### 533. Simulación de clicks de confirmación de mensajes en el socket de Ably
*   **Por qué:** Evitar la desconexión por inactividad confirmando la recepción de tramas de control.
*   **Cómo:** Responder a tramas de confirmación esperadas de Ably de forma automática en el socket.

### 534. Registro de estadísticas de canales de Ably en SQLite
*   **Por qué:** Evaluar qué canales de modelos presentan mayor volumen de datos procesados.
*   **Cómo:** Contabilizar las tramas recibidas agrupadas por canales y guardar el resumen.

### 535. Notificación si la conexión de Ably entra en modo de recuperación (Resume Connection)
*   **Por qué:** Saber si el socket ha recuperado el estado de eventos sin perder mensajes en red.
*   **Cómo:** Detectar tramas de éxito de recuperación (RESUMED) de Ably y registrar el evento.

### 536. Simulación de tramas de desuscripción de Ably en el socket
*   **Por qué:** Dejar de recibir datos de modelos que han salido de watch de forma limpia en red.
*   **Cómo:** Enviar la trama correspondiente de desuscripción al socket asíncronamente.

### 537. Caching de la lista de fallas de canales de Ably en memoria
*   **Por qué:** Evitar intentar reconectarse a canales rechazados por fallas de permisos de forma infinita.
*   **Cómo:** Guardar los canales con fallas en un conjunto de memoria y omitirlos en reintentos.

### 538. Alerta si la conexión de Ably es cerrada por la plataforma por límite de conexiones
*   **Por qué:** Advertir si la cuenta del usuario está superando los límites de sockets simultáneos.
*   **Cómo:** Detectar códigos de error específicos de límite de conexiones en el cierre y alertar.

### 539. Simulación de origen de conexión de Ably desde cliente web móvil
*   **Por qué:** Utilizar flujos de sockets simplificados propios de la versión móvil del sitio.
*   **Cómo:** Enviar la petición de conexión simulando las URIs y cabeceras de red móviles de Ably.

### 540. Caching de logs de conexión de Ably locales para análisis de rendimiento
*   **Por qué:** Disponer de información agregada de estabilidad de red para optimizar reintentos.
*   **Cómo:** Guardar las desconexiones SQLite agrupadas por horas y días en la base local.

### 541. Alerta si la respuesta de auth de Ably carece de la flag de live activa
*   **Por qué:** Saber si el servidor de Chaturbate está rechazando el estado de streaming de la sesión.
*   **Cómo:** Comprobar si la clave `is_live` de los flags de configuración de auth es false y alertar.

### 542. Simulación de tramas de envío de mensajes grupales de Ably en el socket
*   **Por qué:** Enviar mensajes o alertas a grupos de canales simulando la web original.
*   **Cómo:** Enviar tramas de publicación estructuradas de Ably al canal de grupo correspondiente.

### 543. Caching de tokens JWT de Ably en base SQLite locales
*   **Por qué:** Reutilizar tokens válidos entre reinicios rápidos del grabador de forma segura.
*   **Cómo:** Guardar el JWT con su fecha de expiración en las tablas SQLite del grabador.

### 544. Alerta si las cabeceras de Ably carecen de la cabecera de versión del cliente
*   **Por qué:** Evitar rechazos de red enviando la versión de cliente esperada por Ably.
*   **Cómo:** Asegurar que la cabecera `X-Ably-Version` esté configurada en el handshake del socket.

### 545. Simulación de reconexiones a Ably tras cambios de IP pública
*   **Por qué:** Asegurar que el socket se reconecte rápido si el grabador cambia de red o VPN.
*   **Cómo:** Detectar cambios de IP de salida y forzar la re-autenticación y reconexión de Ably.

### 546. Registro de estadísticas de tramas de control de Ably en SQLite
*   **Por qué:** Conocer el overhead de red de las tramas de control de Ably frente a datos de chat.
*   **Cómo:** Almacenar el tamaño y tipo de tramas de control y agruparlas en reportes de red.

### 547. Notificación si la conexión de Ably es rechazada por token inválido
*   **Por qué:** Renovar el token de forma automática solicitando una nueva sesión en la API de auth.
*   **Cómo:** Detectar códigos de error específicos de token expirado en la conexión y disparar auth.

### 548. Simulación de envío de mensajes de presencia en canales de Ably
*   **Por qué:** Registrar la presencia de la sesión del grabador en la lista de viewers de Ably.
*   **Cómo:** Enviar tramas de entrada (ENTER) de presencia al canal correspondiente de Ably.

### 549. Caching de estadísticas de latencia de Ably locales
*   **Por qué:** Optimizar la velocidad del dashboard web de estadísticas de red del grabador.
*   **Cómo:** Guardar los resultados agregados de latencias de Ably en la base de datos de SQLite.

### 550. Alerta si el servidor de Ably devuelve códigos de error de WAF de Cloudflare
*   **Por qué:** Evitar loops de error si el balanceador de Ably bloquea la IP del grabador.
*   **Cómo:** Comprobar si la respuesta del handshake contiene cabeceras de Cloudflare e informar.

---

## 16. WebSocket - Eventos de Sala y Lógica de Negocio

### 551. Registro de chat en formato plano (.txt) mediante `RoomMessageTopic`
*   **Por qué:** Guardar una copia simple y legible del chat de la sala para búsquedas de texto offline.
*   **Cómo:** Capturar tramas de chat en el socket, filtrar el texto limpio y escribirlo en un archivo de logs.

### 552. Alerta inmediata si el modelo inicia transmisión en `RoomStatusTopic`
*   **Por qué:** Lanzar la grabación al segundo exacto de inicio para no perder la introducción del show.
*   **Cómo:** Escuchar el evento de estado y disparar el subproceso grabador si pasa a "public" o "live".

### 553. Marcadores de capítulos en video en base a `RoomTitleChangeTopic`
*   **Por qué:** Navegar por grabaciones extensas saltando directamente a los diferentes shows del día.
*   **Cómo:** Registrar marcas de timestamps al capturar eventos de cambio de título en el socket de Ably.

### 554. Detección de baneo o kick de usuarios en sala mediante `RoomKickTopic`
*   **Por qué:** Documentar eventos de conflicto o sanciones a usuarios molestos en la sala grabada.
*   **Cómo:** Escuchar eventos de baneo en el socket y registrar el usuario sancionado en la base SQLite.

### 555. Detección de silencios de chat de usuarios con `RoomSilenceTopic`
*   **Por qué:** Documentar la censura de mensajes de usuarios en la sala de forma estructurada.
*   **Cómo:** Capturar eventos de silencio en el socket de Ably y registrar el usuario y duración en SQLite.

### 556. Registro de anuncios oficiales en sala mediante `RoomNoticeTopic`
*   **Por qué:** Conservar textos de anuncios importantes y promociones compartidos por el sistema de la sala.
*   **Cómo:** Capturar tramas de Notice en el socket de Ably y guardarlas en tablas SQLite.

### 557. Detección de promociones de viewers con `ViewerPromotionTopic`
*   **Por qué:** Registrar si un espectador ha sido promovido a moderador o fan destacado durante el directo.
*   **Cómo:** Escuchar eventos de promoción en el socket y registrar la fecha y usuario en SQLite.

### 558. Detección de ingresos de miembros al club en `RoomFanClubJoinedTopic`
*   **Por qué:** Registrar el crecimiento de miembros del club de fans del modelo durante la transmisión.
*   **Cómo:** Capturar eventos de join del club en el socket de Ably y guardarlos en SQLite.

### 559. Registro de comandos interactivos cortos en `RoomShortcodeTopic`
*   **Por qué:** Conocer qué comandos interactivas del chat activan acciones en la sala en vivo.
*   **Cómo:** Escuchar eventos de shortcode en el socket y archivar los comandos en las estadísticas.

### 560. Alerta si el balance de tokens de la cuenta cambia en `UserTokenUpdateTopic`
*   **Por qué:** Actualizar el saldo de tokens en la TUI de forma instantánea al enviar propinas.
*   **Cómo:** Capturar eventos de balance en el socket de Ably y actualizar la variable global del saldo.

### 561. Gestión de alertas críticas de la cuenta con `UserAlertTopic`
*   **Por qué:** Advertir al usuario si el servidor envía notificaciones de seguridad a su perfil.
*   **Cómo:** Capturar eventos de alerta en el socket y enviar notificaciones push de alta prioridad.

### 562. Notificación de saldo de tokens bajo en `UserLowBalanceTopic`
*   **Por qué:** Alertar al usuario para que recargue tokens antes de intentar enviar propinas automáticas.
*   **Cómo:** Capturar eventos de balance bajo en el socket de Ably y enviar avisos en la terminal.

### 563. Registro de compras de shows privados en `RoomPurchaseTopic`
*   **Por qué:** Conocer qué usuarios compran accesos especiales o shows privados durante el directo.
*   **Cómo:** Capturar eventos de compra en el socket de Ably y guardarlos en tablas SQLite.

### 564. Detección de cambios de calidad del stream en `QualityUpdateTopic`
*   **Por qué:** Reiniciar la descarga de video si la resolución cambia para capturar siempre la máxima.
*   **Cómo:** Escuchar eventos de calidad en el socket y forzar reconexión de HLS si sube la resolución.

### 565. Monitoreo de latencia del stream de video en `LatencyUpdateTopic`
*   **Por qué:** Conocer la estabilidad del stream reportada por la plataforma en tiempo real.
*   **Cómo:** Capturar eventos de latencia en el socket y guardarlos en las tablas SQLite de red.

### 566. Registro de cambios de estado de sala protegida con contraseña
*   **Por qué:** Evitar loops de error si la sala se cierra con contraseña a mitad de show.
*   **Cómo:** Detectar eventos del tipo `RoomPasswordProtectedTopic` y pausar la descarga de HLS de forma segura.

### 567. Detección de nombramiento de nuevos moderadores en `RoomModeratorPromotedTopic`
*   **Por qué:** Documentar la incorporación de moderadores a la comunidad del modelo en SQLite.
*   **Cómo:** Capturar eventos de promoción de moderador en el socket y guardar usuario en base.

### 568. Detección de remoción de moderadores en `RoomModeratorRevokedTopic`
*   **Por qué:** Registrar la pérdida de permisos de moderadores en la sala grabada de forma cronológica.
*   **Cómo:** Escuchar eventos de revocación en el socket de Ably y registrar usuario en SQLite.

### 569. Registro de actualizaciones de configuraciones en `RoomSettingsTopic`
*   **Por qué:** Documentar cambios en las reglas o restricciones de la sala durante el directo.
*   **Cómo:** Capturar eventos de settings en el socket de Ably y archivar los cambios.

### 570. Alerta si el modelo inicia una meta de propinas en `RoomTipGoalProgressTopic`
*   **Por qué:** Notificar si el creador ha definido una nueva meta de tokens en la sala monitorizada.
*   **Cómo:** Escuchar eventos de progreso de meta en el socket y alertar al usuario si se activa.

### 571. Detección de cambios de menú de propinas en `RoomTipMenuTopic`
*   **Por qué:** Mantener actualizado el menú de propinas del creador para envío automatizado de tokens.
*   **Cómo:** Capturar eventos de menú de propinas en el socket de Ably y guardar en memoria.

### 572. Registro de propinas recibidas en la sala mediante `RoomTipAlertTopic`
*   **Por qué:** Registrar el total de tokens donados durante el directo en las tablas SQLite de estadísticas.
*   **Cómo:** Capturar eventos de propina en el socket de Ably y guardar el monto, usuario y mensaje.

### 573. Alerta si se produce un cambio de backend global de push
*   **Por qué:** Reiniciar la conexión WebSocket de forma limpia antes de que sea interrumpida por el servidor.
*   **Cómo:** Capturar eventos del tipo `GlobalPushServiceBackendChangeTopic` y disparar auth.

### 574. Detección de cambio de subject de la sala en `RoomUpdateTopic`
*   **Por qué:** Registrar variaciones en el tema del show del modelo en las estadísticas de la sesión.
*   **Cómo:** Capturar eventos de actualización en el socket de Ably y guardar en SQLite.

### 575. Monitoreo de entradas y salidas de espectadores con `RoomEnterLeaveTopic`
*   **Por qué:** Evaluar la retención y fluctuación de viewers en la sala a lo largo del directo.
*   **Cómo:** Registrar entradas y salidas del userlist del chat desde eventos de Ably en SQLite.

### 576. Registro de clicks rápidos de propinas de un click en `UserOneClickTopic`
*   **Por qué:** Documentar la interactividad financiera de la cuenta en las estadísticas de SQLite.
*   **Cómo:** Capturar eventos de un click en el socket de Ably y guardar en base local.

### 577. Detección de recargas de tokens automáticas en `UserAutoRefillAttemptTopic`
*   **Por qué:** Informar al usuario en consola si se ha intentado recargar tokens de forma automática.
*   **Cómo:** Capturar eventos de recarga en el socket de Ably y enviar avisos en la terminal.

### 578. Alerta si el color de chat del usuario cambia en `UserColorUpdateTopic`
*   **Por qué:** Actualizar la generación de subtítulos de chat con el color correcto configurado.
*   **Cómo:** Capturar eventos de cambio de color en el socket de Ably y guardar en memoria.

### 579. Registro de apertura de multimedia en chat en `UserChatMediaOpenedTopic`
*   **Por qué:** Documentar el consumo de imágenes o videos compartidos en el chat en las estadísticas.
*   **Cómo:** Capturar eventos de apertura de multimedia en el socket de Ably y guardar en SQLite.

### 580. Detección de remoción de multimedia en chat en `UserChatMediaRemovedTopic`
*   **Por qué:** Registrar la eliminación de archivos compartidos del chat en la línea de tiempo de logs.
*   **Cómo:** Capturar eventos de remoción de multimedia en el socket de Ably y guardar en SQLite.

### 581. Registro de visualización de redes sociales con `UserSMCWatchingTopic`
*   **Por qué:** Conocer si el modelo está interactuando con redes externas en tiempo de show.
*   **Cómo:** Capturar eventos de SMC en el socket de Ably y guardarlos en las tablas SQLite de estadísticas.

### 582. Detección de visualización de noticias del sitio con `UserNewsSeenTopic`
*   **Por qué:** Mantener al día la lectura de avisos sin requerir clicks manuales en la web del usuario.
*   **Cómo:** Capturar eventos de visualización de noticias en el socket de Ably y guardar en base local.

### 583. Notificación de propinas recibidas offline en `OfflineTipNotificationTopic`
*   **Por qué:** Alertar al usuario si la cuenta ha recibido propinas mientras estaba desconectado del directo.
*   **Cómo:** Capturar eventos de propina offline en el socket y enviar notificaciones móviles.

### 584. Registro de actualizaciones de propinas offline en `UpdateOfflineTipNotificationTopic`
*   **Por qué:** Mantener al día la confirmación y cobro de propinas recibidas fuera de directo.
*   **Cómo:** Capturar eventos de actualización en el socket y actualizar la base SQLite local.

### 585. Alerta de notificaciones de campana de nuevos seguidos en `BellNotificationTopic`
*   **Por qué:** Iniciar de forma automática el grabador para creadores recién seguidos en watch.
*   **Cómo:** Capturar eventos de campana en el socket de Ably y lanzar el hilo de descarga de HLS.

### 586. Detección de estado privado del viewer en la sala mediante `RoomUserPrivateStatusTopic`
*   **Por qué:** Cambiar la calidad de grabación si el viewer obtiene acceso exclusivo a un show privado.
*   **Cómo:** Leer el estado de la sala del viewer y forzar reconexión de HLS al playlist premium.

### 587. Detección de estado de cámara oculta activa en `RoomUserHiddenCamStatusTopic`
*   **Por qué:** Grabar transmisiones secundarias u ocultas del modelo asociadas a la sesión principal.
*   **Cómo:** Capturar eventos de cámara oculta en el socket de Ably y lanzar FFmpeg asíncronamente.

### 588. Notificaciones de mensajes del sistema de sala en `RoomUserNoticeTopic`
*   **Por qué:** Documentar avisos específicos enviados por los moderadores del sistema de Chaturbate.
*   **Cómo:** Capturar tramas de Notice del sistema en el socket y guardarlas en SQLite.

### 589. Caching dinámico del listado de espectadores del chat para TUI
*   **Por qué:** Optimizar la velocidad de dibujo de listas de espectadores concurrentes en la terminal.
*   **Cómo:** Guardar los eventos de entrada y salida del userlist en estructuras de conjuntos en RAM.

### 590. Alerta si un moderador elimina un mensaje del chat en caliente
*   **Por qué:** Omitir el mensaje borrado del archivo de subtítulos de chat de la grabación de forma atómica.
*   **Cómo:** Detectar el ID del mensaje borrado en el socket y remover la entrada del archivo SRT.

### 591. Registro de hashtags del stream dinámicamente en cada cambio de subject
*   **Por qué:** Conocer cuáles son las etiquetas más usadas por el modelo a lo largo de las horas.
*   **Cómo:** Leer los hashtags del evento de actualización de subject y agregarlos en SQLite.

### 592. Simulación de reintentos inteligentes ante fallas de cookies en reconexión de Ably
*   **Por qué:** Evitar la pérdida de eventos de chat renovando la sesión antes de reconectar Ably.
*   **Cómo:** Iniciar tareas de refresco de cookies si Ably devuelve códigos de error de autenticación.

### 593. Registro de estadísticas de propinas recibidas por tipo de usuario
*   **Por qué:** Analizar en SQLite qué tipos de usuarios (mods, fanclub, anónimos) donan más al modelo.
*   **Cómo:** Almacenar la propiedad de rol del donante asociada a cada evento de propina en SQLite.

### 594. Notificación si la meta de propinas está cerca de completarse
*   **Por qué:** Conectar para ver en directo el clímax del show cuando la meta está al 90%.
*   **Cómo:** Enviar alerta push al móvil si el valor actual de tokens supera el 90% del total de la meta.

### 595. Registro de la permanencia de usuarios destacados en la sala en SQLite
*   **Por qué:** Conocer cuánto tiempo pasan los mayores donantes en la sala del creador.
*   **Cómo:** Correlacionar tramas de entrada y salida de Ably de usuarios destacados con duraciones de visitas.

### 596. Caching de avatares de usuarios destacados del chat en base SQLite locales
*   **Por qué:** Mostrar fotos de perfil de usuarios influyentes de forma offline en reportes.
*   **Cómo:** Guardar avatares de usuarios con mayor volumen de propinas registradas en SQLite.

### 597. Alerta si el modelo ha deshabilitado el chat general
*   **Por qué:** Informar si la sala restringe la conversación del chat de forma definitiva.
*   **Cómo:** Capturar cambios en las reglas del chat en `RoomSettingsTopic` y notificar al usuario.

### 598. Registro de mensajes con links a redes sociales externas del modelo
*   **Por qué:** Documentar referencias de contacto compartidas por el creador en el chat de directo.
*   **Cómo:** Identificar URLs de redes sociales populares y guardarlas asociadas al video en SQLite.

### 599. Caching de tramas de chat en memoria RAM para optimizar rendimiento de disco
*   **Por qué:** Evitar escrituras en disco en cada línea del chat reduciendo el desgaste de almacenamiento.
*   **Cómo:** Almacenar líneas del chat en RAM y escribirlas en bloque al archivo SRT cada 30 segundos.

### 600. Simulación de click de confirmación de visualización de notificaciones de directo
*   **Por qué:** Cumplir con los flujos de telemetría de interacción del viewer en el sitio de forma realista.
*   **Cómo:** Enviar peticiones automáticas de confirmación de alertas al iniciar la descarga del stream.


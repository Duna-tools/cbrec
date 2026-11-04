# Ramas, revisión y versiones

| Rama | Uso | Integración |
|---|---|---|
| `main` | Código estable y origen de tags finales | Pull request revisado desde `beta` o corrección urgente |
| `beta` | Integración y pruebas de la próxima versión | Pull request desde una rama corta |
| `duna/fix-*`, `duna/feat-*`, `duna/docs-*` | Un cambio concreto | Se retiran manualmente tras integrar si ya no hacen falta |

Las ramas históricas se conservan. No hace falta una tercera rama permanente
mientras no exista una necesidad distinta de estable e integración. Las reglas
remotas requieren PR, checks correctos y conversaciones resueltas, y bloquean
borrados y force-push. Con un único mantenedor no se exige una aprobación externa
que impediría integrar sus propios cambios; al incorporar revisores se elevará
ese requisito. Se permite squash para parches y merge para promover `beta` a `main`.

## Ciclo de trabajo

1. Preparar y probar un cambio pequeño desde `beta`.
2. Revisar el PR y sus resultados; no ocultar errores ni omitir pruebas para integrarlo.
3. Integrar en `beta`. Durante la estabilización, priorizar regresiones y conservación de datos.
4. Preparar una versión preliminar cuando proceda y probar los paquetes correspondientes.
5. Promover `beta` a `main` mediante PR cuando se cumplan los criterios de aceptación.
6. Etiquetar y publicar la versión final solo después de esa promoción.

Tomamos la integración revisada y la estabilización mediante candidatos del
[proceso de Linux](https://kernel.org/doc/html/latest/process/2.Process.html).
La topología de dos ramas es una decisión de cbrec, no una copia de sus árboles
de subsistemas ni de su infraestructura de correo.

## Tags y CD

`Cargo.toml` conserva una versión numérica, por ejemplo `0.0.3`, compatible con
los instaladores MSI. El pipeline acepta únicamente:

- `v0.0.3`: versión estable; el commit debe pertenecer al historial de `main`.
- `v0.0.3-beta.1`: prueba preliminar; el commit debe pertenecer al historial de `beta`.
- `v0.0.3-rc.1`: candidato de estabilización; también se verifica contra `beta`.

Los sufijos empiezan en 1. El tag debe corresponder a la versión base de Cargo.
GitHub marca beta/rc como prerelease y no sustituye Latest. El PKGBUILD convierte
el guion en subrayado para pkgver, conservando el tag original para la URL y la
carpeta de fuentes. Los instaladores nativos muestran la versión base: identifica
una preliminar por su tag/release y usa preferentemente el portable para probarla.
Para pasar de una preliminar instalada a la final con igual versión base puede
ser necesario desinstalar primero el paquete preliminar.

Un push de rama ejecuta CI; no publica una release. El lanzamiento manual produce
artefactos de validación; la publicación requiere un tag explícito y todos los
gates. Crear las ramas no crea tags ni releases. Los paquetes se inspeccionan y
verifican con SHA256SUMS y attestations; esto no reemplaza la QA nativa.

## Comprobaciones requeridas

- Validación inicial: workflow, formato, tag y metadatos del paquete.
- Test (ubuntu-latest): regresiones, cobertura ≥90 %, FFmpeg real y QA PTY.
- Test (macos-latest) y Test (windows-latest): suite portable y ejemplos.
- Security Audit: dependencias comprobadas contra la base de avisos.

No se declaran probados vídeo HLS integrado, todos los protocolos gráficos o todos
los instaladores por el hecho de que una suite portable sea correcta.

## QA de terminal en POSIX

La comprobación conserva la sesión del PTY hasta leer el estado posterior al
cierre de cbrec y compara con una referencia tomada antes de iniciar la TUI.
En macOS se excluye únicamente PENDIN de la igualdad: es un estado transitorio
que [XNU activa al restaurar ICANON](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c),
no una preferencia del usuario. Se siguen verificando el resto de atributos,
control de eco, modo canónico, señales y desactivación de captura de ratón.

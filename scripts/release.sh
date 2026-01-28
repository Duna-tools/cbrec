#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
if [[ -z "$version" || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Uso: $0 X.Y.Z"
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "El arbol de trabajo no esta limpio. Commit o stash antes de continuar."
  exit 1
fi

python3 - "$version" <<'PY'
import re
import sys
from pathlib import Path

version = sys.argv[1]

cargo_toml = Path("Cargo.toml")
contenido = cargo_toml.read_text(encoding="utf-8")
nuevo = re.sub(r'(?m)^version = ".*"$', f'version = "{version}"', contenido, count=1)
if contenido == nuevo:
    raise SystemExit("No se pudo actualizar Cargo.toml")
cargo_toml.write_text(nuevo, encoding="utf-8")

pkgbuild = Path("packaging/arch/PKGBUILD")
contenido = pkgbuild.read_text(encoding="utf-8")
nuevo = re.sub(r'(?m)^pkgver=.*$', f'pkgver={version}', contenido, count=1)
if contenido == nuevo:
    raise SystemExit("No se pudo actualizar PKGBUILD")
pkgbuild.write_text(nuevo, encoding="utf-8")
PY

cargo generate-lockfile

git add Cargo.toml Cargo.lock packaging/arch/PKGBUILD
git commit -m "release: v${version}"
git tag "v${version}"

echo "Listo. Ahora puedes ejecutar:"
echo "  git push origin main"
echo "  git push origin v${version}"

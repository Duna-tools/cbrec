"""Valida el inventario y los archivos comprimidos antes de generar SHA256SUMS."""

import hashlib
import re
from pathlib import Path
import sys
import stat
import tarfile
import tomllib
import unittest
import zipfile


ASSETS = {
    "cbrec-linux-x86_64",
    "cbrec-linux-x86_64.deb",
    "cbrec-linux-x86_64.rpm",
    "cbrec-macos-x86_64.tar.gz",
    "cbrec-macos-aarch64.tar.gz",
    "cbrec-windows-x86_64.exe",
    "cbrec-windows-x86_64.msi",
    "cbrec-windows-x86_64-portable.zip",
    "PKGBUILD",
}


PORTABLE_FILES = {"cbrec.exe", "ffmpeg.exe", "README.md", "LICENSE", "THIRD_PARTY_NOTICES.md"}


def validate_tag(tag, manifest):
    version = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(rf"v{re.escape(version)}(?:-(?:beta|rc)\.[1-9][0-9]*)?", tag):
        raise ValueError(f"El tag {tag!r} no coincide con la versión v{version} de Cargo.toml")


def validate_public_metadata(manifest):
    package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]
    alias = "Duna-tools"
    metadata = package.get("metadata", {})
    if (package.get("authors") != [alias]
            or metadata.get("maintainer") != alias
            or metadata.get("deb", {}).get("maintainer") != alias):
        raise ValueError("Los metadatos de autor y mantenedor deben usar el seudónimo público del proyecto")


def validate_archives(directory):
    for architecture in ("x86_64", "aarch64"):
        with tarfile.open(directory / f"cbrec-macos-{architecture}.tar.gz", "r:gz") as archive:
            members = archive.getmembers()
            if len(members) != 1 or members[0].name != "cbrec":
                raise ValueError("El tar de macOS debe contener únicamente cbrec")
            member = members[0]
            if not member.isfile() or member.size == 0 or not member.mode & 0o111:
                raise ValueError("El ejecutable de macOS debe ser regular, no vacío y ejecutable")
            # Leer el contenido detecta un tar truncado antes de publicar su checksum.
            with archive.extractfile(member) as source:
                hashlib.file_digest(source, "sha256")

    with zipfile.ZipFile(directory / "cbrec-windows-x86_64-portable.zip") as archive:
        members = archive.infolist()
        if len(members) != len(PORTABLE_FILES) or {member.filename for member in members} != PORTABLE_FILES:
            raise ValueError("El portable no contiene exactamente los cinco archivos esperados")
        if any(member.file_size == 0 or stat.S_ISLNK(member.external_attr >> 16) for member in members):
            raise ValueError("El portable contiene archivos vacíos o enlaces")
        if archive.testzip() is not None:
            raise ValueError("El portable contiene un archivo con CRC inválido")
        with archive.open("cbrec.exe") as source, (directory / "cbrec-windows-x86_64.exe").open("rb") as standalone:
            if hashlib.file_digest(source, "sha256").digest() != hashlib.file_digest(standalone, "sha256").digest():
                raise ValueError("El ejecutable portable difiere del ejecutable independiente")


def checksums(directory):
    actual = {path.name for path in directory.iterdir()} - {"SHA256SUMS"}
    if actual != ASSETS:
        raise ValueError(f"Missing: {sorted(ASSETS - actual)}; unexpected: {sorted(actual - ASSETS)}")
    for name in ASSETS:
        path = directory / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Not a nonempty regular file: {name}")
    validate_archives(directory)
    lines = []
    for name in sorted(ASSETS):
        path = directory / name
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        lines.append(f"{digest}  {name}\n")
    return "".join(lines)


def self_test():
    suite = unittest.defaultTestLoader.discover(str(Path(__file__).parent), pattern="test_check_release.py")
    result = unittest.TextTestRunner().run(suite)
    if not result.wasSuccessful():
        raise SystemExit(1)


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif len(sys.argv) == 3 and sys.argv[1] == "--tag":
        validate_tag(sys.argv[2], Path("Cargo.toml"))
        print(f"Tag verificado: {sys.argv[2]}")
    elif sys.argv[1:] == ["--public-metadata"]:
        validate_public_metadata(Path("Cargo.toml"))
        print("Metadatos públicos verificados; el historial de Git requiere una revisión separada")
    elif len(sys.argv) == 2:
        directory = Path(sys.argv[1])
        content = checksums(directory)
        manifest = directory / "SHA256SUMS"
        if manifest.is_symlink():
            raise ValueError("SHA256SUMS no puede ser un enlace simbólico")
        manifest.write_text(content, encoding="utf-8")
    else:
        sys.exit("Uso: python scripts/check_release.py <dist-directory> | --self-test | --tag <tag> | --public-metadata")

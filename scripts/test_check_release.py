"""Regresiones del contrato de publicación, sin ejecutar binarios de terceros."""

from contextlib import nullcontext
import hashlib
import io
from pathlib import Path
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

import check_release


class ReleaseTagChecks(unittest.TestCase):
    def test_public_metadata_rejects_personal_or_missing_identity_fields(self):
        public = ('[package]\nname = "cbrec"\nversion = "0.0.2"\nauthors = ["Duna-tools"]\n'
                  '[package.metadata]\nmaintainer = "Duna-tools"\n'
                  '[package.metadata.deb]\nmaintainer = "Duna-tools"\n')
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / "Cargo.toml"
            manifest.write_text(public)
            check_release.validate_public_metadata(manifest)
            for private in (public.replace('authors = ["Duna-tools"]', 'authors = ["Personal Name"]'),
                            public.replace('maintainer = "Duna-tools"', 'maintainer = "name@example.test"', 1),
                            public.rsplit('maintainer = "Duna-tools"', 1)[0]):
                manifest.write_text(private)
                with self.assertRaises(ValueError):
                    check_release.validate_public_metadata(manifest)

    def test_tag_matches_package_version_instead_of_dependency_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / "Cargo.toml"
            manifest.write_text('[package]\nname = "cbrec"\nversion = "0.0.2"\n[dependencies.anyhow]\nversion = "1"\n')
            check_release.validate_tag("v0.0.2", manifest)
            check_release.validate_tag("v0.0.2-beta.1", manifest)
            check_release.validate_tag("v0.0.2-rc.12", manifest)
            for tag in ("v0.0.1", "0.0.2", "v0.0.2-extra", 'v0.0.2";echo${IFS}bad', "v0.0.2\n"):
                with self.subTest(tag=tag), self.assertRaises(ValueError):
                    check_release.validate_tag(tag, manifest)
            for tag in ("v0.0.2-beta.0", "v0.0.2-beta.01", "v0.0.2-rc", "v0.0.3-beta.1"):
                with self.subTest(tag=tag), self.assertRaises(ValueError):
                    check_release.validate_tag(tag, manifest)

    def test_tag_cli_fails_before_producing_release_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "Cargo.toml").write_text('[package]\nversion = "0.0.2"\n')
            for tag, expected in (("v0.0.2", 0), ("v9.9.9", 1)):
                result = subprocess.run([sys.executable, "-B", check_release.__file__, "--tag", tag],
                                        cwd=directory, capture_output=True)
                self.assertEqual(result.returncode == 0, expected == 0)
                self.assertEqual({path.name for path in directory.iterdir()}, {"Cargo.toml"})


class ReleaseChecks(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        for name in check_release.ASSETS:
            (self.directory / name).write_bytes(b"binary")
        self.portable = self.directory / "cbrec-windows-x86_64-portable.zip"
        self.entries = {
            "cbrec.exe": b"binary",
            "ffmpeg.exe": b"ffmpeg",
            "README.md": b"readme",
            "LICENSE": b"license",
            "THIRD_PARTY_NOTICES.md": b"notices",
        }
        self.write_zip(self.entries)
        for architecture in ("x86_64", "aarch64"):
            self.write_tar(architecture)

    def write_zip(self, entries):
        with zipfile.ZipFile(self.portable, "w") as archive:
            for name, content in entries.items():
                archive.writestr(name, content)

    def write_tar(self, architecture, name="cbrec", kind=tarfile.REGTYPE, mode=0o755, content=b"binary"):
        with tarfile.open(self.directory / f"cbrec-macos-{architecture}.tar.gz", "w:gz") as archive:
            member = tarfile.TarInfo(name)
            member.type = kind
            member.mode = mode
            member.size = len(content) if kind == tarfile.REGTYPE else 0
            member.linkname = "elsewhere" if kind == tarfile.SYMTYPE else ""
            archive.addfile(member, io.BytesIO(content) if member.size else None)

    def test_complete_inventory_is_deterministic(self):
        result = check_release.checksums(self.directory)
        self.assertEqual(result, check_release.checksums(self.directory))
        self.assertEqual(len(result.splitlines()), len(check_release.ASSETS))
        for line in result.splitlines():
            digest, name = line.split("  ", 1)
            self.assertEqual(digest, hashlib.sha256((self.directory / name).read_bytes()).hexdigest())

    def run_cli(self):
        return subprocess.run(
            [sys.executable, "-B", check_release.__file__, str(self.directory)],
            capture_output=True,
            check=False,
        )

    def test_cli_writes_manifest_only_after_validation(self):
        self.assertEqual(self.run_cli().returncode, 0)
        manifest = self.directory / "SHA256SUMS"
        expected = manifest.read_bytes()
        self.assertEqual(expected.decode(), check_release.checksums(self.directory))
        self.portable.write_bytes(b"broken zip")
        self.assertNotEqual(self.run_cli().returncode, 0)
        self.assertEqual(manifest.read_bytes(), expected)

    def test_cli_does_not_follow_manifest_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            external = Path(temporary) / "keep.txt"
            external.write_bytes(b"preserve this data")
            try:
                (self.directory / "SHA256SUMS").symlink_to(external)
            except OSError as error:
                self.skipTest(f"El sistema no permite symlinks: {error}")
            result = self.run_cli()
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(external.read_bytes(), b"preserve this data")

    def test_invalid_archive_is_rejected(self):
        self.portable.write_bytes(b"not a zip")
        with self.assertRaises((ValueError, zipfile.BadZipFile)):
            check_release.checksums(self.directory)

    def test_corrupt_payload_is_rejected(self):
        content = self.portable.read_bytes()
        offset = content.index(b"ffmpeg", content.index(b"ffmpeg.exe") + len(b"ffmpeg.exe"))
        self.portable.write_bytes(content[:offset] + b"X" + content[offset + 1:])
        with self.assertRaises((ValueError, zipfile.BadZipFile)):
            check_release.checksums(self.directory)

    def test_invalid_tar_is_rejected(self):
        (self.directory / "cbrec-macos-aarch64.tar.gz").write_bytes(b"not a tar")
        with self.assertRaises(tarfile.TarError):
            check_release.checksums(self.directory)

    def test_portable_requires_every_file(self):
        for name in self.entries:
            with self.subTest(missing=name):
                self.write_zip({key: value for key, value in self.entries.items() if key != name})
                with self.assertRaises(ValueError):
                    check_release.checksums(self.directory)

    def test_portable_rejects_empty_extra_and_different_binary(self):
        for changes in ({"ffmpeg.exe": b""}, {"../escape": b"x"}, {"cbrec.exe": b"stale binary"}):
            with self.subTest(changes=changes):
                self.write_zip(self.entries | changes)
                with self.assertRaises(ValueError):
                    check_release.checksums(self.directory)

    def test_portable_rejects_duplicate_or_symlink(self):
        for symlink in (False, True):
            self.write_zip({name: content for name, content in self.entries.items() if not symlink or name != "ffmpeg.exe"})
            with zipfile.ZipFile(self.portable, "a") as archive:
                info = zipfile.ZipInfo("ffmpeg.exe" if symlink else "cbrec.exe")
                info.create_system = 3
                info.external_attr = (stat.S_IFLNK | 0o777) << 16
                with self.assertWarns(UserWarning) if not symlink else nullcontext():
                    archive.writestr(info, "target")
            with self.assertRaises(ValueError):
                check_release.checksums(self.directory)

    def test_tar_requires_only_regular_nonempty_executable(self):
        for changes in ({"name": "../cbrec"}, {"kind": tarfile.SYMTYPE}, {"mode": 0o644}, {"content": b""}):
            with self.subTest(changes=changes):
                self.write_tar("x86_64", **changes)
                with self.assertRaises(ValueError):
                    check_release.checksums(self.directory)

    def test_inventory_rejects_missing_empty_directory_and_extra(self):
        asset = self.directory / "PKGBUILD"
        for kind in ("missing", "empty", "directory"):
            with self.subTest(kind=kind):
                asset.unlink()
                if kind == "empty":
                    asset.write_bytes(b"")
                elif kind == "directory":
                    asset.mkdir()
                with self.assertRaises(ValueError):
                    check_release.checksums(self.directory)
                if kind == "directory":
                    asset.rmdir()
                asset.write_bytes(b"binary")
        (self.directory / "unexpected").mkdir()
        with self.assertRaises(ValueError):
            check_release.checksums(self.directory)

    def test_inventory_rejects_symlink_asset(self):
        asset = self.directory / "PKGBUILD"
        asset.unlink()
        try:
            asset.symlink_to(self.directory / "cbrec-linux-x86_64")
        except OSError as error:
            self.skipTest(f"El sistema no permite symlinks: {error}")
        with self.assertRaises(ValueError):
            check_release.checksums(self.directory)


if __name__ == "__main__":
    unittest.main()

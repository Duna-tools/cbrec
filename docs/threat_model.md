# Threat Model / Modelo de Amenazas

## Scope

cbrec is a local CLI recorder. It talks to Chaturbate HTTP endpoints, launches FFmpeg, and writes media files to disk.

In scope:

- User-provided model names and Chaturbate URLs.
- Local configuration files under the cbrec config directory.
- Session cookies supplied by config, environment, or CLI.
- FFmpeg path resolution and child-process execution.
- Output paths, temporary `.part` files, and packaged release artifacts.
- CI/CD packaging and release integrity.

Out of scope:

- Compromise of the user's operating system account.
- Compromise of GitHub, crates.io, Chaturbate, or FFmpeg upstreams.
- DRM bypassing, paywall bypassing, or automated purchases/tipping.
- Guarantees that Chaturbate endpoints remain stable.

## Assets

- Session cookies and authenticated access.
- Recorded media files.
- User filesystem integrity.
- Release artifacts and checksums.
- Availability of unattended `watch` recordings.

## Trust Boundaries

- CLI input crosses into application validation.
- `config.toml` and `watched.toml` cross from local disk into runtime behavior.
- Chaturbate HTTP responses cross from network into parsing and FFmpeg inputs.
- FFmpeg crosses from cbrec into an external executable.
- CI release artifacts cross from build runners to users.

## Main Risks And Current Controls

| Risk | Current control |
|---|---|
| Invalid model input or pasted URLs | Central model normalization through `ModelName` |
| Bad config causing tight loops or disabled recording | Config normalization with warnings |
| Broken local environment | `cbrec doctor` checks FFmpeg, output, watchlist, and auth state |
| File overwrite or partial collision | Recording preflight reserves `.part` files and suffixes colliding outputs |
| FFmpeg path missing or broken | FFmpeg validation before recording |
| FFmpeg stderr leaking cookies | Cookie-looking stderr lines are redacted |
| Chaturbate status changes | HTTP status classification and contract tests |
| Corrupt watched list | Invalid `watched.toml` is backed up and reset |
| Dependency drift | Dependabot for Cargo and GitHub Actions |
| Release integrity | Release workflow publishes `SHA256SUMS` |
| Release provenance | Tagged release assets are covered by GitHub artifact attestations |

## Residual Risks

- Session cookies passed on the command line may be visible in shell history or process listings.
- Chaturbate can change API response shape, access controls, anti-bot behavior, or playlist structure.
- FFmpeg is a large native dependency; cbrec validates execution but does not sandbox FFmpeg.
- CI runners are trusted for release builds.
- Checksums prove integrity after release publication; artifact attestations add build provenance but still trust GitHub-hosted runners.

## Hardening Backlog

- Prefer config/env cookie input in docs over CLI cookies.
- Add more contract tests for playlist variants and blocked responses.
- Add fuzz/property tests for URL/model normalization and playlist parsing.
- Consider signed releases if distribution grows beyond personal use.

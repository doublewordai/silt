# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/doublewordai/silt/compare/v0.1.0...v0.1.1) - 2025-12-12

### Other

- change example. .env
- Merge branch 'main' of https://github.com/doublewordai/silt
- update readme

## [0.1.0](https://github.com/doublewordai/silt/releases/tag/v0.1.0) - 2025-12-12

### Other

- Add required package metadata and MIT license
- Add initial CHANGELOG.md
- Merge pull request #2 from doublewordai/renovate/configure
- Add CARGO_REGISTRY_TOKEN to release-plz workflow
- Initial commit: Silt - transparent batching proxy for OpenAI API

### Added
- Initial release of Silt
- Transparent batching proxy for OpenAI API
- Idempotent request handling with client-generated keys
- Long-lived connection support with TCP keepalives
- Redis-based state management
- Automatic batch processing and polling
- Docker support with multi-stage builds
- GitHub Actions workflows for releases and container publishing

# Release Process

`paperless-cli` now follows the same release shape as
[`damacus/zitadel-tui`](https://github.com/damacus/zitadel-tui), adapted for this
crate.

## What happens on `main`

When changes land on `main`, the release workflow does the following:

1. Runs `release-please` against `release-please-config.json` and
   `.release-please-manifest.json`
2. Creates or updates the release PR and changelog
3. After a release is created, checks out the tagged revision
4. Builds the Linux release binary
5. Packages the binary as `paperless-linux-amd64.tar.gz`
6. Generates `checksums-sha256.txt`
7. Uploads the archive and checksum file to the GitHub release
8. Publishes the crate to crates.io using `cargo publish`

## Required secrets

- `GITHUB_TOKEN`
  Provided automatically by GitHub Actions and used by release-please and
  release asset upload steps.
- `CARGO_REGISTRY_TOKEN`
  Required for the crates.io publish step.

## Required crate metadata

The crate metadata in `Cargo.toml` now includes:

- `description`
- `license`
- `repository`
- `homepage`
- `readme`
- `keywords`
- `categories`

This keeps the crate publish step aligned with crates.io expectations.

## First publish note

This document exists partly to make the first release path explicit and visible
in the repo. Once the PR merges and release-please cuts the first Rust release,
the workflow will package the binary, upload release assets, and attempt the
first crates.io publish automatically.

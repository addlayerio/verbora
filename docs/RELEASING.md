# Releasing Verbora

Verbora uses `release-plz` to prepare and publish releases from GitHub Actions.
The workflow is [`.github/workflows/release.yml`](../.github/workflows/release.yml);
its policy is in [`release-plz.toml`](../release-plz.toml).

## Release flow

1. Merge conventional commits into `main` (`feat:`, `fix:`, `perf:`, etc.).
2. The workflow opens or updates a Release PR for the affected crate or crates.
3. Review each proposed version and its generated release notes.
4. Merge that Release PR.
5. The next workflow run verifies the workspace, publishes the affected
   `verbora-*` crates, and creates their tags and GitHub releases.

`release_always = false` prevents an ordinary push to `main` from publishing a
crate. Only the merge of release-plz's Release PR is a release event.

## Independent versions

Every workspace crate declares its own version. For example, a change limited
to `verbora-distance` produces a release for `verbora-distance` only. If that
release changes a version requirement used by another public Verbora crate,
release-plz includes that dependent crate in the same Release PR so its
published dependency metadata remains valid.

## Manual operation

Use **Actions → Release crates → Run workflow** only to recover a failed run
or refresh the Release PR. Choose `release-pr` to prepare it, or `release` to
process an already-merged Release PR. Do not use `release` to bypass review.

## First public release and trusted publishing

The initial crates.io publication uses `CARGO_REGISTRY_TOKEN`. Once each crate
exists on crates.io, migrate to crates.io trusted publishing and remove that
secret; this avoids retaining a long-lived registry credential.

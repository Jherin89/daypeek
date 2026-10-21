# Packaging

## Homebrew

The live tap is **[Jherin89/homebrew-tap](https://github.com/Jherin89/homebrew-tap)**:

```sh
brew install jherin89/tap/daypeek
```

[`homebrew/daypeek.rb`](homebrew/daypeek.rb) is a copy of the tap's
`Formula/daypeek.rb`, kept here for review. It installs the prebuilt release
binary on Apple Silicon Macs, Intel Macs and x86_64 Linux (static musl build), and
builds the tagged source with Rust on ARM Linux.

### Updating the tap for a new release

1. Tag a release (`git tag vX.Y.Z && git push origin vX.Y.Z`). The
   [release workflow](../.github/workflows/release.yml) uploads
   `daypeek-<target>.tar.gz` and `daypeek-<target>.sha256` for
   `aarch64-apple-darwin`, `x86_64-apple-darwin` and `x86_64-unknown-linux-musl`.
2. In the tap's `Formula/daypeek.rb`, point every `url` at the new tag.
3. Replace each `sha256` with the first field of the matching `.sha256` file:

   ```sh
   curl -fsSL https://github.com/Jherin89/daypeek/releases/download/vX.Y.Z/daypeek-aarch64-apple-darwin.sha256
   ```

4. Check it: `brew style Formula/daypeek.rb`, then
   `brew audit --strict --online jherin89/tap/daypeek`,
   `brew reinstall jherin89/tap/daypeek` and `brew test jherin89/tap/daypeek`.
5. Copy the formula back here.

## install.sh

[`../install.sh`](../install.sh) downloads the same release assets, checks the
SHA-256, and installs into the first of `~/.local/bin`, `~/bin` or
`/usr/local/bin` already on your `PATH` (or `--dir` / `DAYPEEK_INSTALL_DIR`).
If none is on `PATH`, it prints the exact line to fix that. On Linux it prefers
the static musl asset and falls back to the glibc one for older releases. If
there's no asset for the platform, it prints the `cargo install --git` command
instead. CI runs `shellcheck`, a dry run, and a real install into a temp folder
on every push.

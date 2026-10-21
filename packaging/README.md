# Packaging

## Homebrew (template)

[`homebrew/daypeek.rb`](homebrew/daypeek.rb) is a ready-to-fill formula for a
personal tap. There is **no tap repository yet**. To publish one:

1. Tag a release (`git tag v0.1.0 && git push origin v0.1.0`). The
   [release workflow](../.github/workflows/release.yml) uploads
   `daypeek-<target>.tar.gz` and `daypeek-<target>.sha256` for
   `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`.
2. Create a public repo named `homebrew-tap` and copy the formula into it as
   `Formula/daypeek.rb`.
3. Set `version`, and replace each `sha256` placeholder with the first field of
   the matching `.sha256` file:

   ```sh
   curl -fsSL https://github.com/Jherin89/daypeek/releases/download/v0.1.0/daypeek-aarch64-apple-darwin.sha256
   ```

4. Check it locally: `brew install --build-from-source ./Formula/daypeek.rb && brew test daypeek && brew audit --strict daypeek`.

Users then install with `brew install jherin89/tap/daypeek`.

## install.sh

[`../install.sh`](../install.sh) downloads the same release assets, checks the
SHA-256, and installs to `~/.local/bin` (or `/usr/local/bin`). If there's no
release yet, it prints the `cargo install --git` command instead. CI runs
`shellcheck` and a dry run on every push.

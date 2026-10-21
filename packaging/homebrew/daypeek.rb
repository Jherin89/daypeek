# Homebrew formula for daypeek: https://github.com/Jherin89/daypeek
#
# Prebuilt binaries are used where the release has one (v0.1.0: Apple Silicon
# Macs and x86_64 Linux). Everywhere else (Intel Macs, ARM Linux) Homebrew
# builds the same tagged release from source with Rust. Intel Mac and static
# Linux (musl) binaries arrive with the next daypeek release, and this formula
# will switch to them then.
class Daypeek < Formula
  desc "One-screen terminal dashboard: calendar, tasks from 10 apps, weather"
  homepage "https://github.com/Jherin89/daypeek"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.1.0/daypeek-aarch64-apple-darwin.tar.gz"
      sha256 "561031e1e57a47f7a925492e8b003f23c09d7ef80b5d4cb17fbf88182bb748c6"
    end
    on_intel do
      url "https://github.com/Jherin89/daypeek/archive/refs/tags/v0.1.0.tar.gz"
      sha256 "af855dbe904c5d5e07c50431c95b9ead6ea6d958f8a0d67809eb2d985113c371"

      depends_on "rust" => :build
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.1.0/daypeek-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "92f4cf2e22633e5dd077a19e3ae42c144f13baa28f109a16c9b3a66065531723"
    end
    on_arm do
      url "https://github.com/Jherin89/daypeek/archive/refs/tags/v0.1.0.tar.gz"
      sha256 "af855dbe904c5d5e07c50431c95b9ead6ea6d958f8a0d67809eb2d985113c371"

      depends_on "rust" => :build
    end
  end

  def install
    if File.exist?("Cargo.toml")
      # Source tarball (no prebuilt binary for this platform yet).
      system "cargo", "install", *std_cargo_args
    else
      bin.install "daypeek"
    end
  end

  def caveats
    <<~EOS
      Try it with fake data:    daypeek --demo
      Connect your apps:        daypeek setup
      Show it in new terminals: echo 'daypeek --compact' >> ~/.zshrc
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/daypeek --version")
    assert_match "CALENDAR", shell_output("#{bin}/daypeek --demo --color never")
  end
end

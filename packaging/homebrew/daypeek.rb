# Homebrew formula for daypeek: https://github.com/Jherin89/daypeek
#
# Apple Silicon Macs, Intel Macs and x86_64 Linux get the prebuilt release
# binary (static musl build on Linux, so any distribution works). ARM Linux
# builds the same tagged release from source with Rust.
class Daypeek < Formula
  desc "One-screen terminal dashboard: calendar, tasks from 13 apps, weather"
  homepage "https://github.com/Jherin89/daypeek"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.3.0/daypeek-aarch64-apple-darwin.tar.gz"
      sha256 "b62b635b1fc014503c39c068c899dac34cae04d9e3d58f3a7346db5f38a6f907"
    end
    on_intel do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.3.0/daypeek-x86_64-apple-darwin.tar.gz"
      sha256 "3366ebf746f2cc53d0e82f8ffc3260f3f8e2d4a8a65e81768c86ad47df65df96"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.3.0/daypeek-x86_64-unknown-linux-musl.tar.gz"
      sha256 "33bc02968e564c3e30139d3e7c309adfc78bd8eb67fb509d276a65c7f8cb3ce3"
    end
    on_arm do
      url "https://github.com/Jherin89/daypeek/archive/refs/tags/v0.3.0.tar.gz"
      sha256 "2c07ef87df52ae0f3b68b45afa15aaa376e1daddedd8b07feeccc5e3bc5dcac6"

      depends_on "rust" => :build
    end
  end

  def install
    if File.exist?("Cargo.toml")
      # Source tarball (no prebuilt binary for this platform).
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
    assert_match "This week", shell_output("#{bin}/daypeek week --demo --color never")
  end
end

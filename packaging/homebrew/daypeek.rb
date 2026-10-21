# Homebrew formula for daypeek: https://github.com/Jherin89/daypeek
#
# Apple Silicon Macs, Intel Macs and x86_64 Linux get the prebuilt release
# binary (static musl build on Linux, so any distribution works). ARM Linux
# builds the same tagged release from source with Rust.
class Daypeek < Formula
  desc "One-screen terminal dashboard: calendar, tasks from 10 apps, weather"
  homepage "https://github.com/Jherin89/daypeek"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.2.0/daypeek-aarch64-apple-darwin.tar.gz"
      sha256 "5983f3cd19b1610a1c6af5f334703d4ae2158a13d42b537f9ef1dbe69abc0971"
    end
    on_intel do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.2.0/daypeek-x86_64-apple-darwin.tar.gz"
      sha256 "3b15c9991004dd541033082c2794c78ea070f1551034bb2fb96117fc53d11597"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Jherin89/daypeek/releases/download/v0.2.0/daypeek-x86_64-unknown-linux-musl.tar.gz"
      sha256 "953b0470c1332629088505671ec84b7977c1c21255ed535a755d05a5af7c70ea"
    end
    on_arm do
      url "https://github.com/Jherin89/daypeek/archive/refs/tags/v0.2.0.tar.gz"
      sha256 "70a09c352daddb92a64c7bce2957db23e8497cddf647ab3a8864e249f21adeeb"

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

# Homebrew formula TEMPLATE for daypeek.
#
# This file is not used by Homebrew from here. To publish, copy it into a tap
# repository (for example github.com/Jherin89/homebrew-tap as
# Formula/daypeek.rb), then fill in the version and the SHA-256 values from
# the release's *.sha256 files:
#
#   brew tap jherin89/tap && brew install daypeek
#
# Steps for each release are in packaging/README.md.
class Daypeek < Formula
  desc "Fast one-screen daily dashboard: calendar, tasks from 10 apps, and weather"
  homepage "https://github.com/Jherin89/daypeek"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Jherin89/daypeek/releases/download/v#{version}/daypeek-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_daypeek-aarch64-apple-darwin.tar.gz"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/Jherin89/daypeek/releases/download/v#{version}/daypeek-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_daypeek-x86_64-unknown-linux-gnu.tar.gz"
    end
  end

  def install
    bin.install "daypeek"
  end

  def caveats
    <<~EOS
      Get started:
        daypeek --demo    # fake data, no setup
        daypeek setup     # guided setup for your apps

      Show it in every new terminal:
        echo 'daypeek --compact' >> ~/.zshrc
    EOS
  end

  test do
    assert_match "daypeek #{version}", shell_output("#{bin}/daypeek --version")
    assert_match "demo data", shell_output("#{bin}/daypeek --demo --color never")
  end
end

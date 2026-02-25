class Abot < Formula
  desc "AI-native spatial terminal interface"
  homepage "https://github.com/dorky-robot/abot"
  version "VERSION"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dorky-robot/abot/releases/download/vVERSION/abot-vVERSION-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_ARM64"
    end

    on_intel do
      url "https://github.com/dorky-robot/abot/releases/download/vVERSION/abot-vVERSION-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_X86_64"
    end
  end

  def install
    bin.install "abot"
  end

  def post_install
    ohai "abot installed! Run `abot start` to launch."
    ohai "If abot is already running, use `abot update` for zero-downtime upgrade."
  end

  service do
    run [opt_bin/"abot", "start"]
    keep_alive true
    log_path var/"log/abot.log"
    error_log_path var/"log/abot.log"
    working_dir HOMEBREW_PREFIX
  end

  test do
    assert_match "abot", shell_output("#{bin}/abot --help")
  end
end

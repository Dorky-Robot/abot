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

    # Install .app bundle for UTI registration (makes .abot dirs appear as
    # opaque file packages in Finder, like .app or .xcodeproj)
    if OS.mac?
      app_src = buildpath/"abot.app"
      if app_src.exist?
        prefix.install app_src
      end
    end
  end

  def post_install
    if OS.mac?
      # Copy .app to ~/Applications and register the UTI with Launch Services.
      # This makes Finder treat .abot directories as file packages.
      user_apps = Pathname.new(Dir.home)/"Applications"
      user_apps.mkpath
      app_dst = user_apps/"abot.app"
      app_src = prefix/"abot.app"
      if app_src.exist?
        FileUtils.rm_rf(app_dst) if app_dst.exist?
        FileUtils.cp_r(app_src, app_dst)
        system "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
               "-f", app_dst.to_s
        ohai ".abot file type registered — Finder will show .abot bundles as documents"
      end
    end
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

#!/bin/sh
# Write the Homebrew formula for a released abot into a homebrew-tap checkout.
#
#   sh scripts/tap-formula.sh v2.1.0 ~/Projects/dorky_robot/homebrew-tap
#
# Then commit and push the tap. This replaced the release workflow's tap job
# on 2026-10-15: that job needed a cross-repo token Felix decided not to keep.
set -eu
TAG=${1:?tag, e.g. v2.1.0}; TAP=${2:?path to a homebrew-tap checkout}
VERSION=${TAG#v}
BASE="https://github.com/Dorky-Robot/abot/releases/download/${TAG}"
sums=$(curl -fsSL "$BASE/SHA256SUMS")
sha() { echo "$sums" | grep " abot-$1.tar.gz" | cut -d' ' -f1; }
SHA_aarch64_apple_darwin=$(sha aarch64-apple-darwin)
SHA_x86_64_apple_darwin=$(sha x86_64-apple-darwin)
SHA_x86_64_unknown_linux_gnu=$(sha x86_64-unknown-linux-gnu)
SHA_aarch64_unknown_linux_gnu=$(sha aarch64-unknown-linux-gnu)
mkdir -p "$TAP/Formula"
cat > "$TAP/Formula/abot.rb" <<FORMULA
class Abot < Formula
  desc "CLI for a git-based ecosystem log: orient, start, land, dead-end, park, ticket"
  homepage "https://github.com/Dorky-Robot/abot"
  version "${VERSION}"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "${BASE}/abot-aarch64-apple-darwin.tar.gz"
      sha256 "${SHA_aarch64_apple_darwin}"
    end

    on_intel do
      url "${BASE}/abot-x86_64-apple-darwin.tar.gz"
      sha256 "${SHA_x86_64_apple_darwin}"
    end
  end

  on_linux do
    on_arm do
      url "${BASE}/abot-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "${SHA_aarch64_unknown_linux_gnu}"
    end

    on_intel do
      url "${BASE}/abot-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "${SHA_x86_64_unknown_linux_gnu}"
    end
  end

  def install
    bin.install "abot"
  end

  def caveats
    <<~EOS
      abot reads the ecosystem log at ~/Projects/ecosystem-log
      (clone Dorky-Robot/ecosystem-log there, or set ABOT_LOG).
      Begin with: abot orient <project>
    EOS
  end

  test do
    assert_match "orient", shell_output("#{bin}/abot --help")
  end
end
FORMULA
echo "wrote $TAP/Formula/abot.rb for $VERSION"

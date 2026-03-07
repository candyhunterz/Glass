# Glass Terminal - Homebrew Cask Formula
#
# This formula is for a custom Homebrew tap (not the official homebrew-cask repo).
#
# To publish:
#   1. Create a GitHub repo named `homebrew-glass`
#   2. Place this file at `Casks/glass.rb` in that repo
#   3. Users install via: `brew tap <GITHUB_USER>/glass && brew install --cask glass`
#
# For each release:
#   1. Update the `version` field
#   2. Compute SHA256 from the actual DMG: `shasum -a 256 Glass-*.dmg`
#   3. Replace the sha256 value with the computed hash
#
# Note: The official homebrew-cask repo requires notarization (deferred -- PKG-F04).

cask "glass" do
  version "0.1.0"
  sha256 "<SHA256>"

  url "https://github.com/<GITHUB_USER>/glass/releases/download/v#{version}/Glass-#{version}-aarch64.dmg",
      verified: "github.com/<GITHUB_USER>/glass/"
  name "Glass Terminal"
  desc "GPU-accelerated terminal emulator with command structure awareness"
  homepage "https://github.com/<GITHUB_USER>/glass"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "Glass.app"

  zap trash: [
    "~/.glass",
  ]
end

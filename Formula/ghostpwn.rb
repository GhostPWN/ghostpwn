class Ghostpwn < Formula
  desc "Autonomous pentest agent TUI with multi-provider LLM support"
  REPO = "https://github.com/GhostPWN/ghostpwn.git"
  homepage "https://github.com/GhostPWN/ghostpwn"
  version "0.3.1"
  url REPO, tag: "v#{version}", revision: "47b483254946239e7864cb1a840940d17a73d4dd"
  license "MIT"
  head REPO, branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "."
  end

  test do
    assert_match "ghostpwn #{version}", shell_output("#{bin}/ghostpwn --version")
  end
end

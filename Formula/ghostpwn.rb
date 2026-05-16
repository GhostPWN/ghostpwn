class Ghostpwn < Formula
  desc "Autonomous pentest agent TUI with multi-provider LLM support"
  homepage "https://github.com/GhostPWN/ghostpwn"
  url "https://github.com/GhostPWN/ghostpwn.git",
      tag:      "v0.1.7"
  version "0.1.7"
  license "MIT"
  head "https://github.com/GhostPWN/ghostpwn.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "."
  end

  test do
    assert_match "ghostpwn #{version}", shell_output("#{bin}/ghostpwn --version")
  end
end

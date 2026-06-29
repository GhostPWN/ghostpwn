class Ghostpwn < Formula
  desc "Autonomous pentest agent TUI with multi-provider LLM support"
  homepage "https://github.com/GhostPWN/ghostpwn"
  version "0.1.10"
  url "https://github.com/GhostPWN/ghostpwn.git",
      tag:      "v#{version}"
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

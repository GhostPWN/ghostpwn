class Ghostpwn < Formula
  desc "Autonomous pentest agent TUI with multi-provider LLM support"
  REPO = "https://github.com/GhostPWN/ghostpwn.git"
  homepage "https://github.com/GhostPWN/ghostpwn"
  version "0.2.11"
  url REPO, tag: "v#{version}", revision: "6d4751850df93f23628a664faf0afed8aac5b2a3"
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

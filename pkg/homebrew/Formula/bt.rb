class Bt < Formula
  desc "B站终端多功能工具箱 (Bilibili CLI toolbox)"
  homepage "https://github.com/qwerprog/bili-tools"
  version "1.0.1"
  license "MIT"

  on_intel do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.1/bit-v1.0.1-darwin-amd64.zip"
    sha256 "ff10ebc239cf110406bfef7984bc6650692b42ad09179f4c8db6ebc795c8a249"
  end

  on_arm do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.1/bit-v1.0.1-darwin-arm64.zip"
    sha256 "53537b37a43b7d6d6db80837b247e363d75346e8099fa48f1ea2bc5bd82c85fc"
  end

  def install
    bin.install "bit"
  end

  test do
    assert_match "1.0.1", shell_output("#{bin}/bit --version")
  end
end

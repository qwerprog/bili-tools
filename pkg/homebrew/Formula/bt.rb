class Bt < Formula
  desc "B站直播开播工具 — 命令行一键开播/下播"
  homepage "https://github.com/QwerProg/bili-tools"
  version "0.1.5"
  license "MIT"

  on_intel do
    url "https://github.com/QwerProg/bili-tools/releases/download/v0.1.5/bt-x86_64-macos.zip"
    sha256 "0f2e8d3ff538b740c61e7dafc7a252832601de67394cf2d292b12a7053a93ca7"
  end

  on_arm do
    url "https://github.com/QwerProg/bili-tools/releases/download/v0.1.5/bt-arm64-macos.zip"
    sha256 "7f649c020f68c43e1ac3a8cd10cdb23e083f07a6b95ca313c5702cb1c626ff0f"
  end

  def install
    bin.install "bt"
  end
end

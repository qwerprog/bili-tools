class Bt < Formula
  desc "B站直播开播工具 — 命令行一键开播/下播"
  homepage "https://github.com/QwerProg/bili-tools"
  version "0.1.6"
  license "MIT"

  on_intel do
    url "https://github.com/QwerProg/bili-tools/releases/download/v0.1.6/bt-x86_64-macos.zip"
    sha256 "2c388eb06106bd0fc3fd262f5ccdb1cfb00d78a86149a9c87495afba967c3495"
  end

  on_arm do
    url "https://github.com/QwerProg/bili-tools/releases/download/v0.1.6/bt-arm64-macos.zip"
    sha256 "a94b3f64fa0ca53e044253e9c0e21584af27bfc7b04b80dca8a1aecf506bdf77"
  end

  def install
    bin.install "bt"
  end
end

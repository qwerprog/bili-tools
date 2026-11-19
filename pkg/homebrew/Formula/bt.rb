class Bt < Formula
  desc "B站工具箱 — 命令行多功能工具与一键开播/下播"
  homepage "https://github.com/qwerprog/bili-tools"
  version "1.0.0"
  license "MIT"

  on_intel do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.0/bt-x86_64-macos.zip"
    sha256 "08bbe0c112a3bffec16959a7117a0865d01ebfc43b3cfa9d18e5c4b1a6ec4325"
  end

  on_arm do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.0/bt-arm64-macos.zip"
    sha256 "b40a56fb104f6ea887d4fe214477f06637cc99ffcb70da2d2890ceb354d3fc49"
  end

  def install
    bin.install "bt"
  end
end

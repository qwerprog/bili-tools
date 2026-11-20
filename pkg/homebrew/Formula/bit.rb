class Bit < Formula
  desc "B站终端多功能工具箱 (Bilibili CLI toolbox)"
  homepage "https://github.com/qwerprog/bili-tools"
  version "1.0.2"
  license "MIT"

  on_intel do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.2/bit-v1.0.2-darwin-amd64.zip"
    sha256 "53d5fa4e13182b1ab63e837b9a3fb7783a6edbe18fc951f54e68d2e8e28e26ac"
  end

  on_arm do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.2/bit-v1.0.2-darwin-arm64.zip"
    sha256 "7c4c00573fd76bac140f5c3b8df35bc77314e7452402d0ad6ccd3e768eef9a61"
  end

  def install
    bin.install "bit"
  end

  test do
    assert_match "1.0.2", shell_output("#{bin}/bit --version")
  end
end

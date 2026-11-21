class Bit < Formula
  desc "B站终端多功能工具箱 (Bilibili CLI toolbox)"
  homepage "https://github.com/qwerprog/bili-tools"
  version "1.0.3"
  license "MIT"

  on_intel do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.3/bit-v1.0.3-darwin-amd64.zip"
    sha256 "f29840ef44d9ed4c6c2c251dc2f389138bd994d7bd328c06374c4966c7daa26b"
  end

  on_arm do
    url "https://github.com/qwerprog/bili-tools/releases/download/v1.0.3/bit-v1.0.3-darwin-arm64.zip"
    sha256 "beab25c73750ef7dcdfa0c1ea5dd5fad553ba0d11a12779c06cb978315b0d5d5"
  end

  def install
    bin.install "bit"
  end

  test do
    assert_match "1.0.3", shell_output("#{bin}/bit --version")
  end
end

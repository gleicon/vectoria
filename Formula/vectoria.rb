class Vectoria < Formula
  desc "AI-native embedded ecommerce search engine"
  homepage "https://github.com/gleicon/vectoria"
  version "0.1.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/gleicon/vectoria/releases/download/v#{version}/vectoria-macos-arm64.tar.gz"
      sha256 "PLACEHOLDER_ARM64_SHA256"
    end
    on_intel do
      url "https://github.com/gleicon/vectoria/releases/download/v#{version}/vectoria-macos-amd64.tar.gz"
      sha256 "PLACEHOLDER_AMD64_SHA256"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/gleicon/vectoria/releases/download/v#{version}/vectoria-linux-arm64.tar.gz"
      sha256 "PLACEHOLDER_LINUX_ARM64_SHA256"
    end
    on_intel do
      url "https://github.com/gleicon/vectoria/releases/download/v#{version}/vectoria-linux-amd64.tar.gz"
      sha256 "PLACEHOLDER_LINUX_AMD64_SHA256"
    end
  end

  def install
    bin.install "vectoria-server"
    bin.install "vectoria"
  end

  service do
    run [opt_bin/"vectoria-server"]
    keep_alive true
    working_dir var
    log_path var/"log/vectoria.log"
    error_log_path var/"log/vectoria.log"
    environment_variables VECTORIA_STORAGE_PATH: "#{var}/vectoria/vectoria.db"
  end

  test do
    # Start server in background and check health endpoint.
    port = free_port
    pid = fork do
      exec bin/"vectoria-server", "--port", port.to_s
    end
    sleep 2
    assert_match "ok", shell_output("curl -s http://localhost:#{port}/health")
  ensure
    Process.kill("TERM", pid)
    Process.wait(pid)
  end
end

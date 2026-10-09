#!/usr/bin/env bash
# ==============================================================================
# FAST SETUP SCRIPT FOR VAST.AI (CINFERENCE + FAFSTMOBEL / QWEN 3.8 27B)
# Optimized for RTX 5090 (sm_120a) & RTX 4090 (sm_89)
# Target total setup time: 4 - 6 minutes
# ==============================================================================
set -euo pipefail

LOG_FILE="/var/log/fast_setup.log"
exec > >(tee -a "$LOG_FILE") 2>&1

echo "=========================================================="
echo " [START] FAST SETUP INITIALIZING AT $(date)"
echo "=========================================================="

# 1. DETECT GPU ARCHITECTURE
GPU_NAME=$(nvidia-smi --query-gpu=name --format=csv,noheader | head -n1 || echo "Unknown")
echo "[+] Detected GPU: $GPU_NAME"

CUDA_ARCH="120a"
if [[ "$GPU_NAME" == *"5090"* ]]; then
    CUDA_ARCH="120a"
elif [[ "$GPU_NAME" == *"4090"* ]]; then
    CUDA_ARCH="89"
elif [[ "$GPU_NAME" == *"3090"* || "$GPU_NAME" == *"A100"* || "$GPU_NAME" == *"A6000"* ]]; then
    CUDA_ARCH="86"
else
    # Auto query compute capability
    RAW_CAP=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -n1 | tr -d '.' || echo "89")
    CUDA_ARCH="$RAW_CAP"
fi
echo "[+] Target CUDA Architecture: sm_$CUDA_ARCH"

# 2. INSTALL REQUIRED SYSTEM TOOLS IN PARALLEL
echo "[+] Installing system dependencies (aria2, ninja, cmake, ffmpeg)..."
DEBIAN_FRONTEND=noninteractive apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
    aria2 git curl cmake ninja-build ffmpeg libcurl4-openssl-dev build-essential pkg-config >/dev/null

# 3. SETUP REPOSITORY & TOOLS
WORKSPACE="/root/fafstmobel-cinference"
if [[ ! -d "$WORKSPACE" ]]; then
    echo "[+] Cloning satellitedown/fafstmobel-cinference..."
    git clone --depth 1 https://github.com/satellitedown/fafstmobel-cinference.git "$WORKSPACE"
fi
cd "$WORKSPACE"

# Bootstrap uv for fast python environment
if ! command -v uv >/dev/null; then
    echo "[+] Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
fi

echo "[+] Creating Python 3.12 venv..."
uv venv --python 3.12 .venv
PYTHON="$WORKSPACE/.venv/bin/python"
uv pip install --python "$PYTHON" --no-deps -r requirements.lock >/dev/null

# 4. BOTTLENECK FIX 1: ULTRA-FAST PARALLEL MODEL DOWNLOAD (22.9 GB via aria2c)
MODEL_DIR="$WORKSPACE/models/fafstmobel"
MODEL_FILE="$MODEL_DIR/fafstmobel.ninfer"
mkdir -p "$MODEL_DIR"

if [[ ! -f "$MODEL_FILE" || $(stat -c%s "$MODEL_FILE" 2>/dev/null || echo 0) -lt 22000000000 ]]; then
    echo "[+] Downloading 22.9 GB fafstmobel.ninfer with 16 connections via aria2c..."
    aria2c -x 16 -s 16 -k 1M --file-allocation=none --summary-interval=5 \
        --dir="$MODEL_DIR" --out="fafstmobel.ninfer" \
        "https://huggingface.co/satellitedown/fafstmobel/resolve/main/fafstmobel.ninfer"
    echo "[+] Model download completed in record time!"
else
    echo "[+] Model file already verified and present."
fi

# Download small license / metadata files
for lfile in LICENSE-APACHE-2.0 LICENSE.huihui LICENSE.ninfer LICENSE.qwen LICENSE.swift NOTICE; do
    if [[ ! -f "$MODEL_DIR/$lfile" ]]; then
        curl -sL "https://huggingface.co/satellitedown/fafstmobel/raw/main/$lfile" -o "$MODEL_DIR/$lfile" || true
    fi
done

# 5. BOTTLENECK FIX 2: PARALLEL NINJA BUILD FOR TARGET ARCHITECTURE
RUNTIME_DIR="$WORKSPACE/runtime/ninfer"
if [[ ! -f "$RUNTIME_DIR/build/apps/ninfer-serve" ]]; then
    echo "[+] Building Cinference runtime for sm_$CUDA_ARCH using all $(nproc) cores..."
    if [[ ! -d "$RUNTIME_DIR" ]]; then
        git clone --depth 1 https://github.com/satellitedown/cinference.git "$RUNTIME_DIR"
    fi

    # Patch CMakeLists.txt to allow detected CUDA architecture (e.g., 89 for 4090, 120a for 5090)
    sed -i "s/set(CMAKE_CUDA_ARCHITECTURES 120a/set(CMAKE_CUDA_ARCHITECTURES $CUDA_ARCH/g" "$RUNTIME_DIR/CMakeLists.txt"
    sed -i "s/STREQUAL \"120a\"/STREQUAL \"$CUDA_ARCH\"/g" "$RUNTIME_DIR/CMakeLists.txt"
    sed -i "s/\"cuda_architectures\": \"120a\"/\"cuda_architectures\": \"$CUDA_ARCH\"/g" "$WORKSPACE/runtime-manifest.json"

    mkdir -p "$RUNTIME_DIR/build"
    cmake -B "$RUNTIME_DIR/build" -S "$RUNTIME_DIR" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_CUDA_ARCHITECTURES="$CUDA_ARCH"
    
    ninja -C "$RUNTIME_DIR/build" -j "$(nproc)" ninfer-serve
    echo "[+] Build complete!"
fi

# 6. BOTTLENECK FIX 3: AUTOMATED CLOUDFLARE TUNNEL (Zero Manual SSH / Port Forwarding)
echo "[+] Setting up cloudflared tunnel..."
if ! command -v cloudflared >/dev/null; then
    curl -sL https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 -o /usr/local/bin/cloudflared
    chmod +x /usr/local/bin/cloudflared
fi

# Start cloudflared tunnel in background and capture public URL
cloudflared tunnel --url http://127.0.0.1:8001 > /var/log/cloudflared.log 2>&1 &

# Wait for tunnel URL
sleep 3
TUNNEL_URL=$(grep -o 'https://[-a-z0-9]*\.trycloudflare\.com' /var/log/cloudflared.log | head -n1 || echo "")

echo "=========================================================="
echo " [SUCCESS] SETUP FINISHED IN RECORD TIME!"
echo " Public OpenAI API Base: $TUNNEL_URL/v1"
echo " Model Name: fafstmobel-cinference"
echo "=========================================================="
echo "$TUNNEL_URL" > /root/api_tunnel_url.txt

# 7. LAUNCH INFERENCE SERVER
echo "[+] Starting Cinference Server on port 8001..."
exec bash "$WORKSPACE/scripts/serve.sh"

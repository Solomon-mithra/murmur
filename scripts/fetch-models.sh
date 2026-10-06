#!/usr/bin/env bash
# Downloads everything Murmur bundles but git doesn't hold:
#   - Cactus Whistle speech model + the Needle engine (libneedle.a) via Cactus's own CLI
#   - SmolLM2-135M-Instruct from Hugging Face
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"
mkdir -p resources vendor/needle

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "→ Cactus Needle CLI"
python3 -m venv "$tmp/venv"
"$tmp/venv/bin/pip" install -q cactus-needle
(cd "$tmp" && "$tmp/venv/bin/needle" download macos-arm64 >/dev/null && "$tmp/venv/bin/needle" download whistle >/dev/null)
cp "$tmp/macos-arm64/libneedle.a" "$tmp/macos-arm64/needle.h" vendor/needle/
cp "$tmp/whistle.cact" resources/

echo "→ SmolLM2-135M-Instruct"
hf=https://huggingface.co/HuggingFaceTB/SmolLM2-135M-Instruct/resolve/main
curl -fL# -o resources/smollm2-135m.safetensors "$hf/model.safetensors"
curl -fsL -o resources/smollm2-tokenizer.json "$hf/tokenizer.json"
curl -fsL -o resources/smollm2-config.json "$hf/config.json"

echo "✓ models ready in src-tauri/resources"

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
model_name="sherpa-onnx-streaming-zipformer-en-2023-06-26"
model_root="${repo_root}/models"
archive="${model_root}/${model_name}.tar.bz2"
url="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/${model_name}.tar.bz2"

mkdir -p "$model_root"

if [[ ! -d "${model_root}/${model_name}" ]]; then
    echo "Downloading ${model_name}..."
    curl --fail --location --retry 3 --output "$archive" "$url"
    tar -xjf "$archive" -C "$model_root"
    rm -f "$archive"
else
    echo "Model directory already exists: ${model_root}/${model_name}"
fi

required=(
    "encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx"
    "decoder-epoch-99-avg-1-chunk-16-left-128.onnx"
    "joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx"
    "tokens.txt"
)
for file in "${required[@]}"; do
    test -f "${model_root}/${model_name}/${file}" || {
        echo "Missing model file: ${file}" >&2
        exit 1
    }
done

env_file="${repo_root}/.env"
if [[ ! -e "$env_file" ]]; then
    cp "${repo_root}/config/magnolia.env.example" "$env_file"
    echo "Created ${env_file} from config/magnolia.env.example"
else
    echo "Preserving existing ${env_file}"
fi

echo
echo "Sherpa captioning is ready. Run:"
echo "  cd ${repo_root}"
echo "  cargo run -p daemon"

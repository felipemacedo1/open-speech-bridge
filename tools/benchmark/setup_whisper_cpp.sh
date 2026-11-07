#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source_dir="${script_dir}/.cache/whisper.cpp"
model_dir="${script_dir}/.cache/models"
version="v1.9.4"

if [[ ! -d "${source_dir}/.git" ]]; then
  git clone --branch "${version}" --depth 1 https://github.com/ggml-org/whisper.cpp.git "${source_dir}"
fi

cmake -S "${source_dir}" -B "${source_dir}/build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_NATIVE=ON \
  -DGGML_OPENMP=ON \
  -DGGML_CUDA=OFF \
  -DGGML_VULKAN=OFF \
  -DGGML_HIP=OFF \
  -DWHISPER_BUILD_TESTS=OFF
cmake --build "${source_dir}/build" --config Release -j 4

mkdir -p "${model_dir}"
for model in base-q5_1 small-q5_1; do
  "${source_dir}/models/download-ggml-model.sh" "${model}" "${model_dir}"
done

git -C "${source_dir}" rev-parse HEAD

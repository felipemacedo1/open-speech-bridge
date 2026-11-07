#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p "${script_dir}/results"
python3 -m venv "${script_dir}/.venv"
"${script_dir}/.venv/bin/python" -m pip install --upgrade pip
"${script_dir}/.venv/bin/python" -m pip install \
  'torch==2.6.0+cpu' \
  --index-url https://download.pytorch.org/whl/cpu
"${script_dir}/.venv/bin/python" -m pip install -r "${script_dir}/requirements.txt"
"${script_dir}/.venv/bin/python" -m pip freeze > "${script_dir}/results/python-freeze.txt"

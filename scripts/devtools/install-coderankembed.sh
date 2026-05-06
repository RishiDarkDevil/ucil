#!/usr/bin/env bash
# Idempotent installer for the CodeRankEmbed model + tokenizer
# (P2-W8-F02 / WO-0059) — master-plan §18 Phase 2 Week 8 line 1787
# ("CodeRankEmbed (137M, CPU) as default, Qwen3-Embedding (8B, GPU
# optional) as upgrade") + master-plan §4.2 line 303 ("CodeRankEmbed
# (137M params, MIT license, 8K context) ... CPU-friendly, 50-150
# embeddings/sec, ~137MB with Int8 quantization").
#
# Behaviour:
#   1. cd to repo root via `git rev-parse --show-toplevel`.
#   2. Verify each artefact at `ml/models/coderankembed/{model.onnx,
#      tokenizer.json}`; if both already exist with matching sha256,
#      print `[OK]` and exit 0 (idempotent).
#   3. Otherwise download missing/divergent files from a pinned
#      upstream HuggingFace mirror (`lprevelige/coderankembed-onnx-q8`
#      — the Int8-quantised CodeRankEmbed ONNX export weighing
#      138081004 bytes; matches master-plan's "~137MB Int8"
#      expectation).
#   4. Verify sha256 post-download; on mismatch, remove the file and
#      exit 1 with the expected/actual hash pair.
#   5. Optional `shellcheck` lint on this script (silently skipped
#      when shellcheck is not on PATH).
#
# Upstream selection:
#   - `nomic-ai/CodeRankEmbed` (the canonical repo) ships only
#     `model.safetensors`; no ONNX export.
#   - `lprevelige/coderankembed-onnx-q8` is the Int8-quantised
#     ONNX conversion (138 MB) — matches the master-plan target.
#   - `sirasagi62/code-rank-embed-onnx` is the FP32 export (~547 MB)
#     — too large; this script does NOT use it.
#   The exact upstream is documented in the WO-0059 ready-for-review
#   note for verifier audit.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

MODEL_DIR="ml/models/coderankembed"
MODEL_FILE="${MODEL_DIR}/model.onnx"
TOKENIZER_FILE="${MODEL_DIR}/tokenizer.json"

MODEL_URL="https://huggingface.co/lprevelige/coderankembed-onnx-q8/resolve/main/onnx/model.onnx"
TOKENIZER_URL="https://huggingface.co/lprevelige/coderankembed-onnx-q8/resolve/main/tokenizer.json"

# sha256 fingerprints — pinned by the WO-0059 executor at first
# download (2026-05-06). The ETag headers HuggingFace serves are
# xet-hash style and do NOT match sha256; these values are computed
# locally via `sha256sum` against the downloaded bytes.
# gitleaks:allow — these are public file integrity hashes, not secrets.
EXPECTED_MODEL_SHA256="800617daf79153ec525cbe7029ea9e5237923695aa27b68e61ff7bb997a7904c"  # gitleaks:allow
EXPECTED_TOKENIZER_SHA256="91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854"  # gitleaks:allow

mkdir -p "${MODEL_DIR}"

verify_sha256() {
    local file="$1"
    local expected="$2"
    if ! [ -s "${file}" ]; then
        return 1
    fi
    local actual
    actual="$(sha256sum "${file}" | awk '{print $1}')"
    if [ "${actual}" != "${expected}" ]; then
        printf '[FAIL] sha256 mismatch on %s: expected %s, got %s\n' \
            "${file}" "${expected}" "${actual}" >&2
        return 1
    fi
    return 0
}

download_with_sha256() {
    local url="$1"
    local target="$2"
    local expected_sha="$3"
    printf '[INFO] Downloading %s -> %s\n' "${url}" "${target}"
    if ! curl -fL --retry 3 --retry-delay 2 -o "${target}" "${url}"; then
        printf '[FAIL] curl failed for %s\n' "${url}" >&2
        rm -f "${target}"
        return 1
    fi
    if ! verify_sha256 "${target}" "${expected_sha}"; then
        rm -f "${target}"
        return 1
    fi
    printf '[OK] %s sha256 verified\n' "${target}"
    return 0
}

# ── Step 1: idempotency short-circuit ─────────────────────────────────
if verify_sha256 "${MODEL_FILE}" "${EXPECTED_MODEL_SHA256}" 2>/dev/null \
   && verify_sha256 "${TOKENIZER_FILE}" "${EXPECTED_TOKENIZER_SHA256}" 2>/dev/null; then
    printf '[OK] CodeRankEmbed already installed at %s (sha256 matches both files)\n' \
        "${MODEL_DIR}"
    exit 0
fi

# ── Step 2: download missing or divergent artefacts ───────────────────
if ! verify_sha256 "${MODEL_FILE}" "${EXPECTED_MODEL_SHA256}" 2>/dev/null; then
    if ! download_with_sha256 "${MODEL_URL}" "${MODEL_FILE}" "${EXPECTED_MODEL_SHA256}"; then
        exit 1
    fi
fi

if ! verify_sha256 "${TOKENIZER_FILE}" "${EXPECTED_TOKENIZER_SHA256}" 2>/dev/null; then
    if ! download_with_sha256 "${TOKENIZER_URL}" "${TOKENIZER_FILE}" "${EXPECTED_TOKENIZER_SHA256}"; then
        exit 1
    fi
fi

# ── Step 3: optional shellcheck self-lint ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/devtools/install-coderankembed.sh"; then
        printf '[FAIL] shellcheck flagged install-coderankembed.sh\n' >&2
        exit 1
    fi
else
    printf '[INFO] shellcheck not on PATH; skipping shellcheck step.\n'
fi

printf '[OK] CodeRankEmbed installed at %s\n' "${MODEL_DIR}"
exit 0

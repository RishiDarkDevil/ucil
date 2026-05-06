"""Generate the minimal ONNX fixture used by `onnx_inference::test_onnx_session_loads_minimal_model`.

Produces a tiny synthetic graph:

    Inputs:  input_ids: int64[batch=1, seq=*]   (dynamic seq dimension)
    Op:      Cast(to=float32)
    Outputs: embedding: float32[batch=1, seq=*]

The graph satisfies the WO-0058 acceptance contract:

- declares at least one input;
- the first input is named `input_ids`;
- declares at least one output;
- `infer(&[1i64, 2, 3])` produces a non-empty `Vec<f32>` of length 3
  (the Cast op preserves shape; element count = batch * seq = 1 * 3 = 3).

Re-run with `python3 crates/ucil-embeddings/tests/data/build_minimal_onnx.py`
from the workspace root after installing `onnx` (`uv pip install onnx`).
The committed `minimal.onnx` is the bytes produced by this script.

Phase: 2
Feature: P2-W8-F01
Work-order: WO-0058
"""

from __future__ import annotations

from pathlib import Path

import onnx
from onnx import TensorProto, helper


def build_minimal_model() -> onnx.ModelProto:
    """Construct the minimal ONNX model graph in-memory."""
    input_ids = helper.make_tensor_value_info(
        "input_ids",
        TensorProto.INT64,
        ["batch", "seq"],
    )
    embedding = helper.make_tensor_value_info(
        "embedding",
        TensorProto.FLOAT,
        ["batch", "seq"],
    )

    cast_node = helper.make_node(
        "Cast",
        inputs=["input_ids"],
        outputs=["embedding"],
        to=TensorProto.FLOAT,
        name="cast_to_float",
    )

    graph = helper.make_graph(
        nodes=[cast_node],
        name="ucil_minimal",
        inputs=[input_ids],
        outputs=[embedding],
    )

    opset = helper.make_opsetid("", 17)
    model = helper.make_model(
        graph,
        opset_imports=[opset],
        producer_name="ucil-embeddings-test",
    )
    model.ir_version = 8
    onnx.checker.check_model(model)
    return model


def main() -> None:
    out_path = Path(__file__).resolve().parent / "minimal.onnx"
    model = build_minimal_model()
    onnx.save(model, str(out_path))
    print(f"wrote {out_path} ({out_path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()

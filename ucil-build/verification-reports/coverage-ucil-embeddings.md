# Coverage Gate — ucil-embeddings

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-07T21:50:17Z

`cargo test -p ucil-embeddings` failed under coverage instrumentation. Tail of log:

```
test models::qwen3_tests::test_dimension_out_of_range_max_value ... ok
test config::config_tests::test_from_toml_str_returns_default_for_empty_input ... ok
test config::config_tests::test_from_toml_str_returns_toml_error_for_malformed_input ... ok
test models::qwen3_tests::test_gpu_detection_returns_no_gpu_on_default_workspace ... ok
test models::qwen3_tests::test_dimension_out_of_range_value_zero ... ok
test models::qwen3_tests::test_qwen3_error_display_renders_missing_model_file ... ok
test models::qwen3_tests::test_qwen3_error_display_renders_no_gpu ... ok
test config::config_tests::test_from_toml_str_parses_explicit_qwen3 ... ok
test models::qwen3_tests::test_qwen3_error_display_renders_dimension_out_of_range ... ok
test models::tests::coderankembed_error_display_renders_canonical_text ... ok
test models::qwen3_tests::test_qwen3_load_returns_dim_out_of_range_before_gpu_check ... ok
test models::test_coderankembed_inference ... FAILED
test models::tests::load_returns_missing_model_file_for_empty_dir ... ok
test models::tests::pool_and_normalise_clamps_zero_input_to_epsilon ... ok
test models::tests::pool_and_normalise_l2_normalises_correct_length_input ... ok
test models::tests::pool_and_normalise_returns_dim_mismatch_when_too_short ... ok
test models::test_qwen3_config_gate ... ok
test models::tests::pool_and_normalise_returns_dim_mismatch_when_too_long ... ok
test chunker::tests::retokenize_chunk_collapses_oversize_to_signature ... ok
test chunker::tests::collapse_to_signature_handles_single_line_oversize_content ... ok
test models::tests::load_returns_missing_model_file_for_tokenizer_absent ... ok
test onnx_inference::test_onnx_session_loads_minimal_model ... ok
test chunker::tests::chunk_returns_empty_vec_for_empty_source ... ok
test chunker::test_embedding_chunker_real_fixture ... ok

failures:

---- models::test_coderankembed_inference stdout ----

thread 'models::test_coderankembed_inference' (690181) panicked at crates/ucil-embeddings/src/models.rs:920:5:
CodeRankEmbed model artefacts not present at "/home/rishidarkdevil/Desktop/ucil-wt/WO-0068/ml/models/coderankembed"; run `bash scripts/devtools/install-coderankembed.sh` first (P2-W8-F02 / WO-0059); got model.onnx exists=false, tokenizer.json exists=false
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    models::test_coderankembed_inference

test result: FAILED. 36 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

error: test failed, to rerun pass `-p ucil-embeddings --lib`
```

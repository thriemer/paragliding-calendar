pub mod candle;
pub mod clip;
pub mod model2vec;
// ONNX Runtime backend for the int8 UForm v3 multilingual model (the quantized
// ONNX Burn can't import). Linked via `ort` load-dynamic; see ort.rs.
pub mod ort;

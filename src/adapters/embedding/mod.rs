// Disabled with build.rs: the Burn backend `include!`s the ONNX→Burnpack
// codegen output, which OOMs at build time. Superseded by the model2vec backend.
// pub mod burn;
pub mod candle;
pub mod clip;
pub mod model2vec;

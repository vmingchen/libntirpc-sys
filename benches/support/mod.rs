use libntirpc_sys::xdr::{MemoryDecoder, MemoryEncoder};
use std::ffi::CStr;
use std::hint::black_box;

pub(super) fn owned_round_trip(input: &CStr, capacity: usize) {
    let mut encoder = MemoryEncoder::new(capacity).unwrap();
    encoder.encode_string(black_box(input)).unwrap();
    let wire = encoder.as_bytes().to_vec();
    let mut decoder = MemoryDecoder::new(&wire).unwrap();
    let text = decoder.decode_string().unwrap();
    assert_eq!(text.as_c_str(), input);
    black_box(text.as_c_str());
}

pub(super) fn reused_round_trip(
    input: &CStr,
    encoder: &mut MemoryEncoder,
    decoder: &mut MemoryDecoder,
) {
    encoder.reset();
    encoder.encode_string(black_box(input)).unwrap();
    decoder.refill(encoder.as_bytes()).unwrap();
    let text = decoder.decode_string().unwrap();
    assert_eq!(text.as_c_str(), input);
    black_box(text.as_c_str());
}

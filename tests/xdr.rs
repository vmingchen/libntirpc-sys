use crate::*;
use std::ffi::c_char;

fn string_stream(length: u32) -> [u32; 2] {
    [length.to_be(), u32::from_ne_bytes(*b"abc\0")]
}

#[test]
fn truncated_string_preserves_caller_storage() {
    for stream_len in [4, 6, 7] {
        let mut wire = string_stream(3);
        let mut storage = [0x55u8; 8];
        let mut decoded = storage.as_mut_ptr().cast::<c_char>();
        let original = decoded;
        let mut stream: XDR = unsafe { std::mem::zeroed() };
        unsafe {
            xdrmem_ncreate(
                &mut stream,
                wire.as_mut_ptr().cast(),
                stream_len,
                xdr_op_XDR_DECODE,
            );
            assert!(!xdr_wrapstring(&mut stream, &mut decoded));
        }
        assert_eq!(
            decoded, original,
            "failed decoding must preserve caller ownership"
        );
        storage[0] = b'X';
        assert_eq!(storage[0], b'X');
    }
}

#[test]
fn truncated_string_does_not_publish_a_new_allocation() {
    for stream_len in [4, 6, 7] {
        let mut wire = string_stream(3);
        let mut decoded = std::ptr::null_mut();
        let mut stream: XDR = unsafe { std::mem::zeroed() };
        unsafe {
            xdrmem_ncreate(
                &mut stream,
                wire.as_mut_ptr().cast(),
                stream_len,
                xdr_op_XDR_DECODE,
            );
            assert!(!xdr_wrapstring(&mut stream, &mut decoded));
        }
        assert!(decoded.is_null());
    }
}

#[test]
fn complete_string_reuses_caller_storage() {
    let mut wire = string_stream(3);
    let mut storage = [0x55u8; 8];
    let mut decoded = storage.as_mut_ptr().cast::<c_char>();
    let original = decoded;
    let mut stream: XDR = unsafe { std::mem::zeroed() };
    unsafe {
        xdrmem_ncreate(&mut stream, wire.as_mut_ptr().cast(), 8, xdr_op_XDR_DECODE);
        assert!(xdr_wrapstring(&mut stream, &mut decoded));
    }
    assert_eq!(decoded, original);
    assert_eq!(&storage[..4], b"abc\0");
}

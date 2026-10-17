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

fn append_auth(wire: &mut Vec<u8>, flavor: u32, body: &[u8]) {
    wire.extend_from_slice(&flavor.to_be_bytes());
    wire.extend_from_slice(&(body.len() as u32).to_be_bytes());
    wire.extend_from_slice(body);
    wire.resize(wire.len().next_multiple_of(4), 0);
}

#[test]
fn call_encoder_initializes_authentication_padding() {
    for cred_len in [0, 1, 2, 3, 4, 399, 400] {
        for verf_len in [0, 1, 2, 3, 4, 399, 400] {
            let mut message: rpc_msg = unsafe { std::mem::zeroed() };
            message.rm_xid = 123;
            message.rm_direction = msg_type_CALL;
            message.ru.RM_cmb.cb_rpcvers = 2;
            message.cb_prog = 7;
            message.cb_vers = 1;
            message.cb_proc = 1;
            message.cb_cred.oa_flavor = 1;
            message.cb_cred.oa_length = cred_len;
            message.cb_cred.oa_body.fill(0x11);
            message.cb_verf.oa_flavor = 6;
            message.cb_verf.oa_length = verf_len;
            message.cb_verf.oa_body.fill(0x22);

            let mut expected = Vec::new();
            for word in [123u32, 0, 2, 7, 1, 1] {
                expected.extend_from_slice(&word.to_be_bytes());
            }
            append_auth(&mut expected, 1, &vec![0x11; cred_len as usize]);
            append_auth(&mut expected, 6, &vec![0x22; verf_len as usize]);

            let mut storage = [0xabab_ababu32; 256];
            let mut stream: XDR = unsafe { std::mem::zeroed() };
            unsafe {
                xdrmem_ncreate(
                    &mut stream,
                    storage.as_mut_ptr().cast(),
                    std::mem::size_of_val(&storage) as u32,
                    xdr_op_XDR_ENCODE,
                );
                assert!(xdr_ncallmsg(&mut stream, &mut message));
            }
            let actual = unsafe {
                std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), expected.len())
            };
            assert_eq!(
                actual, expected,
                "credential={cred_len}, verifier={verf_len}"
            );
        }
    }
}

#[test]
fn reply_encoder_initializes_verifier_padding() {
    for verifier_len in [0, 1, 2, 3, 4, 399, 400] {
        let mut message: rpc_msg = unsafe { std::mem::zeroed() };
        message.rm_xid = 123;
        message.rm_direction = msg_type_REPLY;
        message.ru.RM_rmb.rp_stat = reply_stat_MSG_ACCEPTED;
        // The zeroed union is initialized as MSG_ACCEPTED above.
        let accepted = unsafe { &mut message.ru.RM_rmb.ru.RP_ar };
        accepted.ar_stat = accept_stat_SUCCESS;
        accepted.ar_verf.oa_flavor = 6;
        accepted.ar_verf.oa_length = verifier_len;
        accepted.ar_verf.oa_body.fill(0x22);
        let mut expected = Vec::new();
        for word in [123u32, 1, 0] {
            expected.extend_from_slice(&word.to_be_bytes());
        }
        append_auth(&mut expected, 6, &vec![0x22; verifier_len as usize]);
        expected.extend_from_slice(&0u32.to_be_bytes());

        let mut storage = [0xabab_ababu32; 128];
        let mut stream: XDR = unsafe { std::mem::zeroed() };
        unsafe {
            xdrmem_ncreate(
                &mut stream,
                storage.as_mut_ptr().cast(),
                std::mem::size_of_val(&storage) as u32,
                xdr_op_XDR_ENCODE,
            );
            assert!(xdr_dplx_msg(&mut stream, &mut message));
        }
        let actual =
            unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), expected.len()) };
        assert_eq!(actual, expected, "verifier={verifier_len}");
    }
}

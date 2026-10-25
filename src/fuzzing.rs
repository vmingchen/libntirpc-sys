//! Safe adapters that exercise libntirpc's XDR and ONC RPC decoders with
//! untrusted bytes.
//!
//! This module is available only with the `fuzzing` feature. These are the
//! same decoders libntirpc runs on data received from the network, so they
//! must tolerate hostile input. Every structure and allocation that crosses
//! the C ABI is owned here, which keeps a fuzz target from reaching libntirpc
//! with an uninitialized pointer.

use std::os::raw::c_void;
use std::ptr;

use crate::{
    rpc_msg,
    xdr::{MemoryDecoder, MemoryEncoder, XdrString},
    xdr_ncallmsg, xdr_nreplymsg, xdr_void,
};

/// Exercise up to 64 refill/rewind/reset/decode operations from one input.
///
/// Every opcode has a little-endian u16 payload length followed by that many
/// bytes (truncated to the remaining input). A fresh decoder is the oracle for
/// each reused decode. Saved string owners are checked after every operation.
pub fn xdr_state(mut data: &[u8]) {
    let mut encoder = MemoryEncoder::new(64).unwrap();
    let mut decoder = MemoryDecoder::new(&[]).unwrap();
    let mut source = Vec::new();
    let mut saved: Vec<(XdrString, Vec<u8>)> = Vec::new();
    for _ in 0..64 {
        let Some((&opcode, rest)) = data.split_first() else {
            break;
        };
        let (payload, remaining) = if rest.len() >= 2 {
            let length = usize::from(u16::from_le_bytes([rest[0], rest[1]])).min(rest.len() - 2);
            rest[2..].split_at(length)
        } else {
            (&[][..], &[][..])
        };
        data = remaining;
        match opcode % 8 {
            0 => {
                decoder.refill(payload).unwrap();
                source.clear();
                source.extend_from_slice(payload);
                assert_eq!(decoder.position(), 0);
            }
            1 => {
                let start = decoder.position();
                assert!(start <= source.len());
                let mut fresh = MemoryDecoder::new(&source[start..]).unwrap();
                let expected = fresh.decode_string();
                let actual = decoder.decode_string();
                assert_eq!(actual.is_ok(), expected.is_ok());
                assert_eq!(decoder.position(), start + fresh.position());
                if let (Ok(actual), Ok(expected)) = (actual, expected) {
                    assert_eq!(actual.as_c_str(), expected.as_c_str());
                    if saved.len() == 8 {
                        saved.remove(0);
                    }
                    saved.push((actual, expected.as_c_str().to_bytes().to_vec()));
                }
            }
            2 => {
                decoder.rewind();
                assert_eq!(decoder.position(), 0);
            }
            3 => {
                let text = payload.split(|byte| *byte == 0).next().unwrap();
                let string = std::ffi::CString::new(text).unwrap();
                let start = encoder.position();
                if encoder.encode_string(&string).is_ok() {
                    let mut wire = Vec::new();
                    wire.extend_from_slice(&(text.len() as u32).to_be_bytes());
                    wire.extend_from_slice(text);
                    wire.resize(wire.len().next_multiple_of(4), 0);
                    assert_eq!(&encoder.as_bytes()[start..], wire);
                } else {
                    // Recover immediately after partial failures, then allow
                    // subsequent operations to keep exercising the same owner.
                    encoder.reset();
                    assert!(encoder.as_bytes().is_empty());
                    encoder.encode_string(c"").unwrap();
                }
                assert!(encoder.position() <= 64);
            }
            4 => {
                encoder.reset();
                assert!(encoder.as_bytes().is_empty());
            }
            5 => {
                decoder.refill(encoder.as_bytes()).unwrap();
                source.clear();
                source.extend_from_slice(encoder.as_bytes());
            }
            6 => {
                // Deterministically visit an oversized-length failure and
                // recover, even when mutation hasn't discovered such inputs.
                decoder.refill(&u32::MAX.to_be_bytes()).unwrap();
                assert!(decoder.decode_string().is_err());
                decoder.rewind();
                assert!(decoder.decode_string().is_err());
                decoder.refill(&[0; 4]).unwrap();
                assert_eq!(decoder.decode_string().unwrap().as_c_str(), c"");
                source.clear();
                source.extend_from_slice(&[0; 4]);
            }
            _ => {
                // Destroy/recreate streams while keeping decoded allocations.
                decoder = MemoryDecoder::new(&source).unwrap();
                encoder = MemoryEncoder::new(64).unwrap();
            }
        }
        for (value, expected) in &saved {
            assert_eq!(value.as_c_str().to_bytes(), expected);
        }
    }
}

/// Decode `data` as an RPC call message, an RPC reply message, and a counted
/// XDR string.
///
/// libntirpc reports malformed input by returning `false` from these
/// routines; no input may crash, abort, or leak.
pub fn rpc_xdr(data: &[u8]) {
    let Ok(mut stream) = MemoryDecoder::new(data) else {
        return;
    };
    decode_call(&mut stream);
    stream.rewind();
    decode_reply(&mut stream);
    stream.rewind();
    std::hint::black_box(stream.decode_string().ok());
}

/// Decode a CALL message. `xdr_ncallmsg` writes only into the caller-provided
/// `rpc_msg` (the credential and verifier use inline fixed-size storage), so
/// `XDR_FREE` is a no-op and nothing outlives the call.
fn decode_call(xdrs: &mut MemoryDecoder) {
    // SAFETY: rpc_msg's C integer, pointer, and union fields admit zero. The
    // call decoder initializes the CALL arm and uses inline auth storage.
    let mut msg: rpc_msg = unsafe { std::mem::zeroed() };
    // SAFETY: The stream and its backing allocation remain live and unmoved;
    // msg is exclusively owned and large enough for the decoded CALL fields.
    let ok = unsafe { xdr_ncallmsg(xdrs.as_raw(), &mut msg) };
    std::hint::black_box((ok, &msg));
}

/// Decode a REPLY message. On `accept_stat == SUCCESS` the decoder invokes
/// `AR_results.proc`, exactly as a real client does, so install a no-op result
/// decoder before handing over the message.
fn decode_reply(xdrs: &mut MemoryDecoder) {
    // SAFETY: rpc_msg's fields admit zero; initialize the SUCCESS result
    // callback below before C can inspect that union arm.
    let mut msg: rpc_msg = unsafe { std::mem::zeroed() };
    msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.proc_ = Some(xdr_void);
    msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.where_ = ptr::null_mut::<c_void>();
    // SAFETY: The stream/backing storage and writable msg are live. The result
    // callback is xdr_void, which accepts a null result pointer and retains none
    // of these addresses; verifier storage is inline in msg.
    let ok = unsafe { xdr_nreplymsg(xdrs.as_raw(), &mut msg) };
    std::hint::black_box((ok, &msg));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_recovers_and_retains_strings_across_refills() {
        let mut operations = Vec::new();
        for (opcode, payload) in [
            (3, &b"a greeting"[..]),
            (5, &[][..]),
            (1, &[][..]),
            (2, &[][..]),
            (1, &[][..]),
            (0, &b"\0\0\0\x03a"[..]),
            (1, &[][..]),
            (6, &[][..]),
            (7, &[][..]),
            (3, &[b'x'; 128][..]),
            (4, &[][..]),
            (3, &b"ok"[..]),
            (5, &[][..]),
            (1, &[][..]),
        ] {
            operations.push(opcode);
            operations.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            operations.extend_from_slice(payload);
        }
        xdr_state(&operations);
    }

    #[test]
    fn independent_decodes_can_run_concurrently() {
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..32 {
                        rpc_xdr(&[0, 0, 0, 3, b'a', b'b', b'c', 0]);
                        rpc_xdr(&[0xff; 4]);
                    }
                });
            }
        });
    }
}

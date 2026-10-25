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

use crate::{rpc_msg, xdr::MemoryDecoder, xdr_ncallmsg, xdr_nreplymsg, xdr_void};

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

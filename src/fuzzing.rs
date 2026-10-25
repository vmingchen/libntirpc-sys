//! Safe adapters that exercise libntirpc's XDR and ONC RPC decoders with
//! untrusted bytes.
//!
//! This module is available only with the `fuzzing` feature. These are the
//! same decoders libntirpc runs on data received from the network, so they
//! must tolerate hostile input. Every structure and allocation that crosses
//! the C ABI is owned here, which keeps a fuzz target from reaching libntirpc
//! with an uninitialized pointer.

use std::os::raw::{c_char, c_void};
use std::ptr;

use crate::{
    XDR, rpc_msg, u_int, xdr_ncallmsg, xdr_nreplymsg, xdr_op_XDR_DECODE, xdr_void, xdr_wrapstring,
    xdrmem_ncreate,
};

/// Decode `data` as an RPC call message, an RPC reply message, and a counted
/// XDR string.
///
/// libntirpc reports malformed input by returning `false` from these
/// routines; no input may crash, abort, or leak.
pub fn rpc_xdr(data: &[u8]) {
    decode_call(data);
    decode_reply(data);
    decode_string(data);
}

fn memory_stream(data: &[u8]) -> (Vec<u32>, Box<XDR>) {
    // `xdrmem_ncreate` aborts unless the backing store is 32-bit aligned, and
    // a `Vec<u8>` only guarantees one-byte alignment. Storing the bytes in a
    // `Vec<u32>` gives the required alignment while keeping the exact byte
    // sequence the decoder expects.
    // `xdrmem_ncreate` takes a 32-bit length; never let a larger slice wrap.
    let length = data.len().min(u_int::MAX as usize) as u_int;
    let mut buffer: Vec<u32> = data[..length as usize]
        .chunks(4)
        .map(|chunk| {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            u32::from_ne_bytes(word)
        })
        .collect();
    // Keep a real aligned allocation even for an empty input.
    if buffer.is_empty() {
        buffer.push(0);
    }
    // SAFETY: XDR contains C integers, raw pointers and nullable function
    // pointers, all of which admit zero. C initializes the stream before use.
    let mut xdrs: Box<XDR> = Box::new(unsafe { std::mem::zeroed() });
    // SAFETY: The u32 allocation is aligned, initialized, and at least length
    // bytes long. Both allocations remain live through decoding. XDR's x_base
    // points into itself, so boxing it before initialization keeps that address
    // stable when this function returns. The memory stream owns no C resources
    // (xdrmem_destroy is a no-op); neither allocation is resized by callers.
    unsafe {
        xdrmem_ncreate(
            &mut *xdrs,
            buffer.as_mut_ptr().cast::<c_char>(),
            length,
            xdr_op_XDR_DECODE,
        );
    }
    (buffer, xdrs)
}

/// Decode a CALL message. `xdr_ncallmsg` writes only into the caller-provided
/// `rpc_msg` (the credential and verifier use inline fixed-size storage), so
/// `XDR_FREE` is a no-op and nothing outlives the call.
fn decode_call(data: &[u8]) {
    let (_buffer, mut xdrs) = memory_stream(data);
    // SAFETY: rpc_msg's C integer, pointer, and union fields admit zero. The
    // call decoder initializes the CALL arm and uses inline auth storage.
    let mut msg: rpc_msg = unsafe { std::mem::zeroed() };
    // SAFETY: The stream and its backing allocation remain live and unmoved;
    // msg is exclusively owned and large enough for the decoded CALL fields.
    let ok = unsafe { xdr_ncallmsg(&mut *xdrs, &mut msg) };
    std::hint::black_box((ok, &msg));
}

/// Decode a REPLY message. On `accept_stat == SUCCESS` the decoder invokes
/// `AR_results.proc`, exactly as a real client does, so install a no-op result
/// decoder before handing over the message.
fn decode_reply(data: &[u8]) {
    let (_buffer, mut xdrs) = memory_stream(data);
    // SAFETY: rpc_msg's fields admit zero; initialize the SUCCESS result
    // callback below before C can inspect that union arm.
    let mut msg: rpc_msg = unsafe { std::mem::zeroed() };
    msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.proc_ = Some(xdr_void);
    msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.where_ = ptr::null_mut::<c_void>();
    // SAFETY: The stream/backing storage and writable msg are live. The result
    // callback is xdr_void, which accepts a null result pointer and retains none
    // of these addresses; verifier storage is inline in msg.
    let ok = unsafe { xdr_nreplymsg(&mut *xdrs, &mut msg) };
    std::hint::black_box((ok, &msg));
}

/// Decode a counted string and release it through libntirpc's own free stream
/// so the `mem_alloc`/`mem_free` allocator pair stays matched. The declared
/// length is attacker-controlled but `xdr_wrapstring` caps it at
/// `RPC_MAXDATASIZE`.
fn decode_string(data: &[u8]) {
    let (_buffer, mut xdrs) = memory_stream(data);
    let mut string: *mut c_char = ptr::null_mut();
    // SAFETY: The stream/backing storage stay live, and the null output lets
    // ntirpc allocate the decoded string. On failure it frees its allocation
    // without publishing it; on success the result is NUL-terminated.
    if unsafe { xdr_wrapstring(&mut *xdrs, &mut string) } && !string.is_null() {
        // SAFETY: XDR's C fields admit zero, and XDR_FREE needs only x_op.
        let mut freeing: XDR = unsafe { std::mem::zeroed() };
        freeing.x_op = crate::xdr_op_XDR_FREE;
        // SAFETY: string is the live allocation returned above, owned here;
        // XDR_FREE uses the matching ntirpc allocator and clears the pointer.
        // Use a local stream so concurrent fuzz calls never share mutable state.
        unsafe {
            xdr_wrapstring(&mut freeing, &mut string);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_stream_keeps_its_self_pointer_after_returning() {
        for data in [&[][..], &[0, 0, 0, 3, b'a', b'b', b'c', 0][..]] {
            let (buffer, stream) = memory_stream(data);
            assert_eq!(stream.x_base.cast_const(), (&raw const stream.x_v).cast());
            assert_eq!(stream.x_data.cast_const(), buffer.as_ptr().cast());
            rpc_xdr(data);
        }
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

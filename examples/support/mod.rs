//! Shared FFI callbacks for the hello examples.

use libntirpc_sys::*;
use std::ffi::{c_char, c_void};

/// Adapt the generic XDR callback to `xdr_wrapstring`.
///
/// # Safety
/// `xdrs` must be a live initialized stream. `arg` must be an exclusive writable
/// pointer to a `*mut c_char`: readable and NUL-terminated for encoding, null or
/// sufficiently large writable storage for decoding, or an owned ntirpc string
/// for freeing. All storage must outlive the synchronous call.
pub(super) unsafe extern "C" fn wrap_string(xdrs: *mut XDR, arg: *mut c_void) -> bool {
    // SAFETY: The caller supplies the stream and pointer-to-string required by
    // xdr_wrapstring; this changes only the callback's erased argument type.
    unsafe { xdr_wrapstring(xdrs, arg.cast::<*mut c_char>()) }
}

/// Allocate request storage; ntirpc owns the returned Box until svc_req_free.
pub(super) extern "C" fn svc_req_alloc(xprt: *mut SVCXPRT, xdrs: *mut XDR) -> *mut svc_req {
    // SAFETY: svc_req consists of C integers, raw pointers, nullable callbacks
    // and unions, all admitting zero. No pointed-to memory is accessed here.
    // Box aborts on allocation failure instead of dereferencing a null calloc.
    let mut req: Box<svc_req> = Box::new(unsafe { std::mem::zeroed() });
    req.rq_xprt = xprt;
    req.rq_xdrs = xdrs;
    Box::into_raw(req)
}

/// Return a request allocation to Rust.
///
/// # Safety
/// `req` must be the live pointer returned by svc_req_alloc, released exactly
/// once after ntirpc and its callbacks have finished accessing the request.
pub(super) unsafe extern "C" fn svc_req_free(req: *mut svc_req, _stat: xprt_stat) {
    // SAFETY: The callback contract returns exclusive ownership of the Box
    // created above; svc_req's pointer fields have no Rust-owned destructors.
    unsafe { drop(Box::from_raw(req)) }
}

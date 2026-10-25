//! Safe synchronous string-to-string RPC calls.
//!
//! This convenience API owns process-wide client initialization. It uses
//! AUTH_NONE and ntirpc's default timeout/retries and cached per-thread clients.
//! Initialization runs once; independent calls can run concurrently. Runtime
//! resources remain live for the process lifetime: dropping a reply never shuts
//! down ntirpc while cached clients or worker threads may still use it.
//!
//! Raw FFI users must not shut down/reconfigure this runtime, change allocator
//! hooks, or concurrently invoke raw `rpc_call` while this API is in use.

use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

use crate::xdr::XdrString;
use crate::{XDR, clnt_stat, rpcproc_t, rpcprog_t, rpcvers_t};

/// Failure to initialize the runtime or complete a string RPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Initialization failed or conflicts with an existing raw-FFI runtime.
    InitializationFailed,
    /// ntirpc returned this RPC status.
    CallFailed(clnt_stat),
    /// The call succeeded without decoding a string.
    MissingResult,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializationFailed => {
                formatter.write_str("ntirpc client initialization failed")
            }
            Self::CallFailed(status) => write!(formatter, "RPC call failed with status {status}"),
            Self::MissingResult => formatter.write_str("RPC call returned no string"),
        }
    }
}

impl std::error::Error for Error {}

static INITIALIZED: OnceLock<bool> = OnceLock::new();

fn initialize() -> bool {
    *INITIALIZED.get_or_init(|| {
        // SAFETY: This C struct contains scalars, raw pointers, and nullable
        // callbacks, all admitting zero. Required fields are set before use.
        let mut params: crate::svc_init_params = unsafe { std::mem::zeroed() };
        params.flags = (crate::SVC_INIT_EPOLL | crate::SVC_INIT_NOREG_XPRTS) as crate::u_long;
        params.max_events = 16;
        params.alloc_cb = Some(allocate_request);
        params.free_cb = Some(free_request);
        // SAFETY: OnceLock serializes our initialization. Callbacks
        // have static lifetimes and matched Box allocation/free. EPOLL supplies
        // valid event capacity; NOREG prevents dispatch before client callbacks
        // are installed. Raw FFI users must preserve this runtime's invariants.
        unsafe { crate::svc_init(&mut params) }
    })
}

/// Call an RPC procedure whose argument and result are XDR wrapstrings.
///
/// `host` and `transport` are NUL-terminated names (for example `c"127.0.0.1"`
/// and `c"udp"`). Input is borrowed until the synchronous call returns; the
/// returned string owns its ntirpc allocation and frees it on drop, including
/// decoded storage left behind by a failed call. Strings use C semantics and
/// need not be UTF-8. Both ends must agree on the string-to-string wire format.
///
/// This initializes a process-lifetime client runtime on first use. It does not
/// support custom raw-FFI server initialization in the same process. Calls are
/// independent after one-time initialization. An in-progress reply decoder may
/// extend a call beyond ntirpc's nominal timeout; it finishes before return.
///
/// ```no_run
/// let reply = libntirpc_sys::client::call_string(
///     c"127.0.0.1", 0x3000_0001, 1, 1, c"Alice", c"udp",
/// )?;
/// println!("{}", reply.as_c_str().to_string_lossy());
/// # Ok::<(), libntirpc_sys::client::Error>(())
/// ```
pub fn call_string(
    host: &CStr,
    program: rpcprog_t,
    version: rpcvers_t,
    procedure: rpcproc_t,
    input: &CStr,
    transport: &CStr,
) -> Result<XdrString, Error> {
    if !initialize() {
        return Err(Error::InitializationFailed);
    }
    let mut argument = input;
    let mut result: Option<XdrString> = None;
    // SAFETY: The runtime is initialized; ntirpc caches clients per thread. The
    // paired callbacks below receive exactly their expected argument types.
    // C strings and stack slots remain live until clnt_req_finish_sync has
    // removed the request and drained its active/queued reply callbacks.
    // Encoding only reads input; decoding publishes uniquely owned strings.
    let status = unsafe {
        crate::rpc_call(
            host.as_ptr(),
            program,
            version,
            procedure,
            Some(encode_string),
            (&raw mut argument).cast(),
            Some(decode_string),
            (&raw mut result).cast(),
            transport.as_ptr(),
        )
    };
    if status != crate::clnt_stat_RPC_SUCCESS {
        return Err(Error::CallFailed(status)); // Drop any decoded result too.
    }
    result.ok_or(Error::MissingResult)
}

/// # Safety
/// stream must be a live, exclusively accessed ntirpc stream. argument must
/// point to a live &CStr slot supplied by call_string for this synchronous call.
unsafe extern "C" fn encode_string(stream: *mut XDR, argument: *mut c_void) -> bool {
    // SAFETY: The callback contract supplies an initialized readable stream.
    if unsafe { (*stream).x_op } != crate::xdr_op_XDR_ENCODE {
        return false;
    }
    // SAFETY: call_string supplies exactly this live slot. The borrowed string
    // remains live and immutable throughout encoding; ENCODE never frees it.
    let mut pointer = unsafe { *argument.cast::<&CStr>() }.as_ptr().cast_mut();
    // SAFETY: The stream is in ENCODE mode and pointer is a readable terminated
    // string. Only this local pointer slot is mutable; no ownership transfers.
    unsafe { crate::xdr_wrapstring(stream, &mut pointer) }
}

/// # Safety
/// stream must have valid callbacks and live backing storage for its bounds,
/// exclusively accessed during this call. argument points to the writable
/// Option<XdrString> slot supplied by call_string; no other user accesses it.
unsafe extern "C" fn decode_string(stream: *mut XDR, argument: *mut c_void) -> bool {
    // SAFETY: ntirpc supplies the live stream under the callback contract. The
    // constructor checks DECODE mode and frees unpublished storage on failure.
    let decoded = unsafe { XdrString::decode_from_raw(stream) };
    match decoded {
        Ok(value) => {
            // SAFETY: The callback contract grants exclusive access to this
            // initialized slot. Replacing a previous result drops it correctly.
            unsafe { *argument.cast::<Option<XdrString>>() = Some(value) };
            true
        }
        Err(_) => false,
    }
}

extern "C" fn allocate_request(xprt: *mut crate::SVCXPRT, xdrs: *mut XDR) -> *mut crate::svc_req {
    // SAFETY: This C struct's scalars, pointers, unions and nullable callbacks
    // admit zero. No pointee is accessed; required pointers are assigned below.
    let mut request: Box<crate::svc_req> = Box::new(unsafe { std::mem::zeroed() });
    request.rq_xprt = xprt;
    request.rq_xdrs = xdrs;
    Box::into_raw(request)
}

/// # Safety
/// request must be a live Box returned by allocate_request, exclusively handed
/// back once by ntirpc after all users have finished accessing the request.
unsafe extern "C" fn free_request(request: *mut crate::svc_req, _status: crate::xprt_stat) {
    // SAFETY: The callback contract transfers the unique allocation back to Rust.
    unsafe { drop(Box::from_raw(request)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::UnsafeCell;

    struct Stream {
        descriptor: Box<UnsafeCell<XDR>>,
        _buffer: Vec<u32>,
    }

    impl Stream {
        fn new(bytes: &[u8], mode: crate::xdr_op) -> Self {
            let mut buffer = vec![0u32; bytes.len().div_ceil(4).max(1)];
            for (index, byte) in bytes.iter().enumerate() {
                let mut word = buffer[index / 4].to_ne_bytes();
                word[index % 4] = *byte;
                buffer[index / 4] = u32::from_ne_bytes(word);
            }
            // SAFETY: XDR's C scalars, pointers and nullable callbacks admit zero.
            let descriptor = Box::new(UnsafeCell::new(unsafe { std::mem::zeroed::<XDR>() }));
            // SAFETY: Both allocations remain stable and live inside Self. The
            // initialized aligned buffer covers bytes.len(); only fixed modes
            // ENCODE/DECODE are passed by these tests. Memory destroy is a no-op.
            unsafe {
                crate::xdrmem_ncreate(
                    descriptor.get(),
                    buffer.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    mode,
                );
            }
            Self {
                descriptor,
                _buffer: buffer,
            }
        }
    }

    #[test]
    fn callbacks_round_trip_and_reject_wrong_modes() {
        let mut input = c"hello";
        let mut result: Option<XdrString> = None;
        let stream = Stream::new(&[0; 12], crate::xdr_op_XDR_ENCODE);
        // SAFETY: The stream is live and exclusively used here; the slots have
        // the exact types required by the callbacks and outlive each invocation.
        unsafe {
            assert!(encode_string(
                stream.descriptor.get(),
                (&raw mut input).cast()
            ));
            assert!(!decode_string(
                stream.descriptor.get(),
                (&raw mut result).cast()
            ));
        }
        let stream = Stream::new(
            &[0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o', 0, 0, 0],
            crate::xdr_op_XDR_DECODE,
        );
        // SAFETY: Same live stream/slot contracts as above, now in DECODE mode.
        unsafe {
            assert!(!encode_string(
                stream.descriptor.get(),
                (&raw mut input).cast()
            ));
            assert!(decode_string(
                stream.descriptor.get(),
                (&raw mut result).cast()
            ));
        }
        drop(stream);
        assert_eq!(result.unwrap().as_c_str(), c"hello");
    }

    #[test]
    fn failed_decode_preserves_owned_result_and_can_retry() {
        let wire = [0, 0, 0, 3, b'a', b'b', b'c', 0];
        let mut result: Option<XdrString> = None;
        for length in [8, 6, 8] {
            let stream = Stream::new(&wire[..length], crate::xdr_op_XDR_DECODE);
            // SAFETY: Each stream owns initialized aligned storage; result is
            // exclusively borrowed for this callback and remains live afterward.
            let success =
                unsafe { decode_string(stream.descriptor.get(), (&raw mut result).cast()) };
            assert_eq!(success, length == 8);
            assert_eq!(result.as_ref().unwrap().as_c_str(), c"abc");
        }
        // The successful retry replaced and freed the original owned result.
    }
}

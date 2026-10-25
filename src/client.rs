//! Safe synchronous RPC calls with reusable client configuration and typed codecs.
//!
//! This convenience API owns process-wide client initialization. It uses
//! AUTH_NONE and ntirpc's default timeout/retries and cached per-thread clients.
//! Initialization runs once; independent calls can run concurrently. Runtime
//! resources remain live for the process lifetime: dropping a reply never shuts
//! down ntirpc while cached clients or worker threads may still use it.
//!
//! Raw FFI users must not shut down/reconfigure this runtime, change allocator
//! hooks, or concurrently invoke raw `rpc_call` while this API is in use.

use std::any::Any;
use std::cell::Cell;
use std::ffi::{CStr, CString, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::OnceLock;

use crate::xdr::{self, XdrDecoder, XdrEncoder, XdrString};
use crate::{XDR, clnt_stat, rpcproc_t, rpcprog_t, rpcvers_t};

/// Failure to initialize the runtime or complete an RPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Initialization failed or conflicts with an existing raw-FFI runtime.
    InitializationFailed,
    /// ntirpc returned this RPC status.
    CallFailed(clnt_stat),
    /// The call succeeded without decoding a reply.
    MissingResult,
    /// The request codec failed.
    EncodeFailed(xdr::Error),
    /// The reply codec failed.
    DecodeFailed(xdr::Error),
    /// A codec tried to start another managed RPC on its current thread.
    ReentrantCall,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializationFailed => {
                formatter.write_str("ntirpc client initialization failed")
            }
            Self::CallFailed(status) => write!(formatter, "RPC call failed with status {status}"),
            Self::MissingResult => formatter.write_str("RPC call returned no reply"),
            Self::EncodeFailed(error) => write!(formatter, "RPC request encoding failed: {error}"),
            Self::DecodeFailed(error) => write!(formatter, "RPC reply decoding failed: {error}"),
            Self::ReentrantCall => {
                formatter.write_str("nested RPC calls from codecs are unsupported")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EncodeFailed(error) | Self::DecodeFailed(error) => Some(error),
            _ => None,
        }
    }
}

/// The wire codec and number of an RPC within a client's program/version.
///
/// Implementations use safe borrowed XDR streams: raw pointers and C callbacks
/// stay inside this crate. Codecs may be invoked more than once on retries.
/// Requests must be shareable and replies movable between threads because
/// ntirpc may run callbacks on worker threads. Replies cannot borrow the stream.
///
/// Partially constructed values are dropped on failure using ordinary Rust
/// ownership. A panic is caught at the C boundary and resumed on the caller
/// after ntirpc has drained its callbacks (`panic = "abort"` still aborts).
/// Calling another managed RPC on the callback thread returns [`Error::ReentrantCall`].
///
/// ```
/// use std::ffi::CStr;
/// use libntirpc_sys::{client::RpcCodec, rpcproc_t};
/// use libntirpc_sys::xdr::{self, XdrDecoder, XdrEncoder, XdrString};
///
/// struct Hello;
/// impl RpcCodec for Hello {
///     const NUMBER: rpcproc_t = 1;
///     type Request = CStr;
///     type Reply = XdrString;
///
///     fn encode(xdr: &mut XdrEncoder<'_>, name: &CStr) -> Result<(), xdr::Error> {
///         xdr.encode_string(name)
///     }
///     fn decode(xdr: &mut XdrDecoder<'_>) -> Result<XdrString, xdr::Error> {
///         xdr.decode_string()
///     }
/// }
/// ```
pub trait RpcCodec {
    /// Procedure number within the client's RPC program and version.
    const NUMBER: rpcproc_t;
    /// Borrowed input, kept live through all encoding callbacks.
    /// Thread-local interior mutation cannot be shared with a worker:
    ///
    /// ```compile_fail
    /// use std::cell::Cell;
    /// use libntirpc_sys::{client::RpcCodec, xdr::{self, XdrDecoder, XdrEncoder}};
    /// struct NotShared;
    /// impl RpcCodec for NotShared {
    ///     const NUMBER: u32 = 1;
    ///     type Request = Cell<u32>;
    ///     type Reply = u32;
    ///     fn encode(xdr: &mut XdrEncoder<'_>, value: &Cell<u32>) -> Result<(), xdr::Error> {
    ///         xdr.encode_u32(value.get())
    ///     }
    ///     fn decode(xdr: &mut XdrDecoder<'_>) -> Result<u32, xdr::Error> {
    ///         xdr.decode_u32()
    ///     }
    /// }
    /// ```
    type Request: ?Sized + Sync;
    /// Owned decoded output, transferred from the decoder to the calling thread.
    /// Values tied to one thread cannot be returned by a codec:
    ///
    /// ```compile_fail
    /// use std::rc::Rc;
    /// use libntirpc_sys::{client::RpcCodec, xdr::{self, XdrDecoder, XdrEncoder}};
    /// struct NotMovable;
    /// impl RpcCodec for NotMovable {
    ///     const NUMBER: u32 = 1;
    ///     type Request = u32;
    ///     type Reply = Rc<u32>;
    ///     fn encode(xdr: &mut XdrEncoder<'_>, value: &u32) -> Result<(), xdr::Error> {
    ///         xdr.encode_u32(*value)
    ///     }
    ///     fn decode(xdr: &mut XdrDecoder<'_>) -> Result<Rc<u32>, xdr::Error> {
    ///         Ok(Rc::new(xdr.decode_u32()?))
    ///     }
    /// }
    /// ```
    type Reply: Send;

    /// Encode a request directly onto ntirpc's output stream.
    fn encode(xdr: &mut XdrEncoder<'_>, request: &Self::Request) -> Result<(), xdr::Error>;
    /// Decode a reply directly from ntirpc's input stream.
    fn decode(xdr: &mut XdrDecoder<'_>) -> Result<Self::Reply, xdr::Error>;
}

/// Built-in string-to-string codec for procedure `NUMBER`.
///
/// Both directions use `xdr_wrapstring`, preserving C string semantics and
/// non-UTF-8 bytes. ntirpc limits each string to 9000 bytes.
pub struct WrapString<const NUMBER: rpcproc_t>;

impl<const NUMBER: rpcproc_t> RpcCodec for WrapString<NUMBER> {
    const NUMBER: rpcproc_t = NUMBER;
    type Request = CStr;
    type Reply = XdrString;

    fn encode(xdr: &mut XdrEncoder<'_>, request: &CStr) -> Result<(), xdr::Error> {
        xdr.encode_string(request)
    }

    fn decode(xdr: &mut XdrDecoder<'_>) -> Result<XdrString, xdr::Error> {
        xdr.decode_string()
    }
}

/// Reusable configuration for synchronous calls to one RPC program/version.
///
/// Owns the host and transport names; construction copies them once. Connections
/// are established lazily and cached per thread by ntirpc. This is not a unique
/// connection handle: dropping it does not close ntirpc's cached connections or
/// stop the process-lifetime runtime. AUTH_NONE and default timeouts/retries apply.
/// Calls borrow requests without copying and encode directly onto network XDR.
/// Independent calls may share this client across threads.
///
/// ```no_run
/// use libntirpc_sys::client::{Client, WrapString};
/// let client = Client::new(c"127.0.0.1", 0x3000_0001, 1, c"udp")?;
/// let reply = client.call::<WrapString<1>>(c"Alice")?;
/// println!("{}", reply.as_c_str().to_string_lossy());
/// # Ok::<(), libntirpc_sys::client::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Client {
    host: CString,
    program: rpcprog_t,
    version: rpcvers_t,
    transport: CString,
}

impl Client {
    /// Initialize the managed runtime and copy the endpoint configuration.
    /// No connection is attempted until the first call.
    pub fn new(
        host: &CStr,
        program: rpcprog_t,
        version: rpcvers_t,
        transport: &CStr,
    ) -> Result<Self, Error> {
        if !initialize() {
            return Err(Error::InitializationFailed);
        }
        Ok(Self {
            host: host.to_owned(),
            program,
            version,
            transport: transport.to_owned(),
        })
    }

    /// Invoke the procedure identified by `C`, using its request/reply codec.
    /// Both ends must agree on the wire format. A reply decoder finishes before
    /// return, even if it runs past ntirpc's nominal timeout.
    pub fn call<C: RpcCodec>(&self, request: &C::Request) -> Result<C::Reply, Error> {
        call::<C>(
            &self.host,
            self.program,
            self.version,
            C::NUMBER,
            request,
            &self.transport,
        )
    }
}

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
/// For reusable endpoint configuration and custom codecs, use [`Client::call`].
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
    call::<WrapString<0>>(host, program, version, procedure, input, transport)
}

thread_local! {
    // Protect rpc_call's per-thread cache from codec-triggered recursion.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

struct CallScope(bool);

impl CallScope {
    fn enter() -> Result<Self, Error> {
        if ACTIVE.with(|active| active.replace(true)) {
            return Err(Error::ReentrantCall);
        }
        Ok(Self(false))
    }

    fn callback() -> Self {
        Self(ACTIVE.with(|active| active.replace(true)))
    }
}

impl Drop for CallScope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(self.0));
    }
}

type Panic = Box<dyn Any + Send>;

struct EncodeState<'a, C: RpcCodec> {
    request: &'a C::Request,
    error: Option<xdr::Error>,
    panic: Option<Panic>,
}

struct DecodeState<C: RpcCodec> {
    reply: Option<C::Reply>,
    error: Option<xdr::Error>,
    panic: Option<Panic>,
}

impl<C: RpcCodec> Default for DecodeState<C> {
    fn default() -> Self {
        Self {
            reply: None,
            error: None,
            panic: None,
        }
    }
}

fn call<C: RpcCodec>(
    host: &CStr,
    program: rpcprog_t,
    version: rpcvers_t,
    procedure: rpcproc_t,
    request: &C::Request,
    transport: &CStr,
) -> Result<C::Reply, Error> {
    let _scope = CallScope::enter()?;
    if !initialize() {
        return Err(Error::InitializationFailed);
    }
    let mut argument = EncodeState::<C> {
        request,
        error: None,
        panic: None,
    };
    let mut result = DecodeState::<C>::default();
    // SAFETY: The runtime is initialized; ntirpc caches clients per thread. The
    // paired callbacks below receive exactly their expected argument types.
    // Endpoint strings and separate state slots stay live until clnt_req_finish_sync has
    // removed the request and drained its active/queued reply callbacks.
    // Encoding borrows a Sync request; worker-thread decoding produces a Send
    // reply. ntirpc serializes each slot's callbacks, and drains them before
    // returning. No Rust access races C; panics cannot unwind through C.
    let status = unsafe {
        crate::rpc_call(
            host.as_ptr(),
            program,
            version,
            procedure,
            Some(encode::<C>),
            (&raw mut argument).cast(),
            Some(decode::<C>),
            (&raw mut result).cast(),
            transport.as_ptr(),
        )
    };
    finish::<C>(argument, result, status)
}

fn finish<C: RpcCodec>(
    mut argument: EncodeState<'_, C>,
    mut result: DecodeState<C>,
    status: clnt_stat,
) -> Result<C::Reply, Error> {
    if let Some(panic) = argument.panic.take().or_else(|| result.panic.take()) {
        resume_unwind(panic);
    }
    if status != crate::clnt_stat_RPC_SUCCESS {
        if let Some(error) = argument.error {
            return Err(Error::EncodeFailed(error));
        }
        if let Some(error) = result.error {
            return Err(Error::DecodeFailed(error));
        }
        return Err(Error::CallFailed(status)); // Drop any decoded result too.
    }
    result.reply.ok_or(Error::MissingResult)
}

/// # Safety
/// stream must be a live, exclusively accessed ntirpc stream. argument must
/// point to this call's live EncodeState<C> slot, with callbacks serialized.
unsafe extern "C" fn encode<C: RpcCodec>(stream: *mut XDR, argument: *mut c_void) -> bool {
    // SAFETY: call::<C> supplies exactly this live, exclusively used state slot.
    let state = unsafe { &mut *argument.cast::<EncodeState<'_, C>>() };
    if state.panic.is_some() {
        return false;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        let _scope = CallScope::callback();
        // SAFETY: ntirpc supplies an exclusive live stream for this callback.
        let mut stream = unsafe { XdrEncoder::from_raw(stream) }?;
        C::encode(&mut stream, state.request)
    })) {
        Ok(result) => {
            state.error = result.err();
            state.error.is_none()
        }
        Err(panic) => {
            state.panic = Some(panic);
            false
        }
    }
}

/// # Safety
/// stream must have valid callbacks and live backing storage for its bounds,
/// exclusively accessed during this call. argument points to the writable
/// DecodeState<C> slot supplied by call::<C>; no other user accesses it.
unsafe extern "C" fn decode<C: RpcCodec>(stream: *mut XDR, argument: *mut c_void) -> bool {
    // SAFETY: call::<C> supplies the matching initialized exclusive state slot.
    let state = unsafe { &mut *argument.cast::<DecodeState<C>>() };
    if state.panic.is_some() {
        return false;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        let _scope = CallScope::callback();
        // SAFETY: ntirpc supplies an exclusive live stream for this callback.
        let mut stream = unsafe { XdrDecoder::from_raw(stream) }?;
        let value = C::decode(&mut stream)?;
        // Keep replacement/drop inside catch_unwind: Reply may have a custom Drop.
        state.reply = Some(value);
        Ok(())
    })) {
        Ok(result) => {
            state.error = result.err();
            state.error.is_none()
        }
        Err(panic) => {
            state.panic = Some(panic);
            false
        }
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
            assert!(matches!(
                mode,
                crate::xdr_op_XDR_ENCODE | crate::xdr_op_XDR_DECODE
            ));
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

        fn encode<C: RpcCodec>(&self, state: &mut EncodeState<'_, C>) -> bool {
            // SAFETY: This test stream and the matching typed state stay live
            // and exclusively used during the synchronous callback.
            unsafe { encode::<C>(self.descriptor.get(), (&raw mut *state).cast()) }
        }

        fn decode<C: RpcCodec>(&self, state: &mut DecodeState<C>) -> bool {
            // SAFETY: This test stream and the matching typed state stay live
            // and exclusively used during the synchronous callback.
            unsafe { decode::<C>(self.descriptor.get(), (&raw mut *state).cast()) }
        }
    }

    #[test]
    fn callbacks_round_trip_and_reject_wrong_modes() {
        let mut input = EncodeState::<WrapString<1>> {
            request: c"hello",
            error: None,
            panic: None,
        };
        let mut result = DecodeState::<WrapString<1>>::default();
        let stream = Stream::new(&[0; 12], crate::xdr_op_XDR_ENCODE);
        assert!(stream.encode(&mut input));
        assert!(!stream.decode(&mut result));
        assert_eq!(result.error, Some(xdr::Error::WrongMode));
        let stream = Stream::new(
            &[0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o', 0, 0, 0],
            crate::xdr_op_XDR_DECODE,
        );
        assert!(!stream.encode(&mut input));
        assert_eq!(input.error, Some(xdr::Error::WrongMode));
        assert!(stream.decode(&mut result));
        assert_eq!(result.error, None);
        drop(stream);
        assert_eq!(result.reply.unwrap().as_c_str(), c"hello");
    }

    #[test]
    fn failed_decode_preserves_owned_result_and_can_retry() {
        let wire = [0, 0, 0, 3, b'a', b'b', b'c', 0];
        let mut result = DecodeState::<WrapString<1>>::default();
        for length in [8, 6, 8] {
            let stream = Stream::new(&wire[..length], crate::xdr_op_XDR_DECODE);
            let success = stream.decode(&mut result);
            assert_eq!(success, length == 8);
            assert_eq!(result.reply.as_ref().unwrap().as_c_str(), c"abc");
        }
        // The successful retry replaced and freed the original owned result.
    }

    struct NumberAndName;

    impl RpcCodec for NumberAndName {
        const NUMBER: rpcproc_t = 7;
        type Request = (u32, CString);
        type Reply = (u32, XdrString);

        fn encode(xdr: &mut XdrEncoder<'_>, request: &Self::Request) -> Result<(), xdr::Error> {
            xdr.encode_u32(request.0)?;
            xdr.encode_string(&request.1)
        }

        fn decode(xdr: &mut XdrDecoder<'_>) -> Result<Self::Reply, xdr::Error> {
            Ok((xdr.decode_u32()?, xdr.decode_string()?))
        }
    }

    #[test]
    fn custom_codec_round_trips_directly_over_borrowed_streams() {
        let input = (u32::MAX, c"\xffname".to_owned());
        let mut encoder = xdr::MemoryEncoder::new(32).unwrap();
        NumberAndName::encode(&mut encoder.encoder(), &input).unwrap();
        assert_eq!(&encoder.as_bytes()[..4], &u32::MAX.to_be_bytes());
        let mut decoder = xdr::MemoryDecoder::new(encoder.as_bytes()).unwrap();
        let reply = NumberAndName::decode(&mut decoder.decoder()).unwrap();
        drop(encoder);
        drop(decoder);
        assert_eq!(reply.0, input.0);
        assert_eq!(reply.1.as_c_str(), input.1.as_c_str());
    }

    #[test]
    fn reply_moves_from_worker_to_caller_after_stream_destruction() {
        let result = std::thread::spawn(|| {
            let stream = Stream::new(&[0, 0, 0, 3, b'a', b'b', b'c', 0], crate::xdr_op_XDR_DECODE);
            let mut result = DecodeState::<WrapString<1>>::default();
            assert!(stream.decode(&mut result));
            result
        })
        .join()
        .unwrap();
        let argument = EncodeState::<WrapString<1>> {
            request: c"abc",
            error: None,
            panic: None,
        };
        let reply = finish(argument, result, crate::clnt_stat_RPC_SUCCESS).unwrap();
        assert_eq!(reply.as_c_str(), c"abc");
    }

    #[test]
    fn callback_failures_return_codec_errors_and_missing_results_are_rejected() {
        let argument = || EncodeState::<WrapString<1>> {
            request: c"abc",
            error: None,
            panic: None,
        };
        let mut input = argument();
        assert!(!Stream::new(&[0; 4], crate::xdr_op_XDR_ENCODE).encode(&mut input));
        assert!(matches!(
            finish(
                input,
                DecodeState::default(),
                crate::clnt_stat_RPC_CANTENCODEARGS
            ),
            Err(Error::EncodeFailed(xdr::Error::OperationFailed)),
        ));
        let mut result = DecodeState::<WrapString<1>>::default();
        assert!(!Stream::new(&[0, 0, 0, 3], crate::xdr_op_XDR_DECODE).decode(&mut result));
        assert!(matches!(
            finish(argument(), result, crate::clnt_stat_RPC_CANTDECODERES),
            Err(Error::DecodeFailed(xdr::Error::OperationFailed)),
        ));
        assert!(matches!(
            finish(
                argument(),
                DecodeState::default(),
                crate::clnt_stat_RPC_SUCCESS
            ),
            Err(Error::MissingResult),
        ));
    }

    #[test]
    fn partial_replies_and_replies_from_failed_calls_are_dropped() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DROPS: AtomicUsize = AtomicUsize::new(0);

        struct Reply(XdrString);
        impl Drop for Reply {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::Relaxed);
            }
        }
        struct Owned;
        impl RpcCodec for Owned {
            const NUMBER: rpcproc_t = 1;
            type Request = CStr;
            type Reply = Reply;

            fn encode(xdr: &mut XdrEncoder<'_>, request: &CStr) -> Result<(), xdr::Error> {
                xdr.encode_string(request)
            }

            fn decode(xdr: &mut XdrDecoder<'_>) -> Result<Reply, xdr::Error> {
                let reply = Reply(xdr.decode_string()?);
                xdr.decode_u32()?;
                Ok(reply)
            }
        }
        let wire = [0, 0, 0, 3, b'a', b'b', b'c', 0, 0, 0, 0, 42];
        let mut result = DecodeState::<Owned>::default();
        assert!(!Stream::new(&wire[..8], crate::xdr_op_XDR_DECODE).decode(&mut result));
        assert!(result.reply.is_none());
        assert_eq!(DROPS.load(Ordering::Relaxed), 1);
        assert!(Stream::new(&wire, crate::xdr_op_XDR_DECODE).decode(&mut result));
        assert!(result.error.is_none());
        assert_eq!(result.reply.as_ref().unwrap().0.as_c_str(), c"abc");
        let input = EncodeState::<Owned> {
            request: c"abc",
            error: None,
            panic: None,
        };
        assert!(matches!(
            finish(input, result, crate::clnt_stat_RPC_FAILED),
            Err(Error::CallFailed(crate::clnt_stat_RPC_FAILED)),
        ));
        assert_eq!(DROPS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn panicking_reply_drop_cannot_unwind_through_c() {
        struct Reply(bool);
        impl Drop for Reply {
            fn drop(&mut self) {
                if self.0 {
                    panic!("reply drop panic");
                }
            }
        }
        struct DropPanic;
        impl RpcCodec for DropPanic {
            const NUMBER: rpcproc_t = 1;
            type Request = u32;
            type Reply = Reply;

            fn encode(xdr: &mut XdrEncoder<'_>, value: &u32) -> Result<(), xdr::Error> {
                xdr.encode_u32(*value)
            }

            fn decode(xdr: &mut XdrDecoder<'_>) -> Result<Reply, xdr::Error> {
                Ok(Reply(xdr.decode_u32()? != 0))
            }
        }
        let mut result = DecodeState::<DropPanic>::default();
        assert!(Stream::new(&1u32.to_be_bytes(), crate::xdr_op_XDR_DECODE).decode(&mut result));
        // Replacing the first result drops it inside the C callback. Rust installs
        // the new non-panicking value even when dropping the old one unwinds.
        assert!(!Stream::new(&0u32.to_be_bytes(), crate::xdr_op_XDR_DECODE).decode(&mut result));
        assert!(result.panic.is_some());
        let input = EncodeState::<DropPanic> {
            request: &0,
            error: None,
            panic: None,
        };
        let panic = catch_unwind(AssertUnwindSafe(|| {
            finish(input, result, crate::clnt_stat_RPC_CANTDECODERES)
        }))
        .err()
        .unwrap();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"reply drop panic"));
    }

    struct Panicking;

    impl RpcCodec for Panicking {
        const NUMBER: rpcproc_t = 1;
        type Request = CStr;
        type Reply = XdrString;

        fn encode(_: &mut XdrEncoder<'_>, _: &CStr) -> Result<(), xdr::Error> {
            panic!("encoder panic")
        }

        fn decode(xdr: &mut XdrDecoder<'_>) -> Result<XdrString, xdr::Error> {
            let _partial = xdr.decode_string()?;
            panic!("decoder panic")
        }
    }

    #[test]
    fn encoder_panic_is_retained_until_return_to_rust_and_blocks_retries() {
        let mut input = EncodeState::<Panicking> {
            request: c"abc",
            error: None,
            panic: None,
        };
        let stream = Stream::new(&[0; 16], crate::xdr_op_XDR_ENCODE);
        assert!(!stream.encode(&mut input));
        assert!(input.panic.is_some());
        assert!(!stream.encode(&mut input));
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _scope = CallScope::enter().unwrap();
            finish(
                input,
                DecodeState::default(),
                crate::clnt_stat_RPC_CANTENCODEARGS,
            )
        }))
        .err()
        .unwrap();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"encoder panic"));
        assert!(CallScope::enter().is_ok());
    }

    #[test]
    fn decoder_panic_on_worker_is_resumed_on_calling_thread() {
        let result = std::thread::spawn(|| {
            let stream = Stream::new(&[0, 0, 0, 3, b'a', b'b', b'c', 0], crate::xdr_op_XDR_DECODE);
            let mut result = DecodeState::<Panicking>::default();
            assert!(!stream.decode(&mut result));
            assert!(result.panic.is_some());
            assert!(!stream.decode(&mut result));
            assert!(CallScope::enter().is_ok());
            result
        })
        .join()
        .unwrap();
        let input = EncodeState::<Panicking> {
            request: c"abc",
            error: None,
            panic: None,
        };
        let panic = catch_unwind(AssertUnwindSafe(|| {
            finish(input, result, crate::clnt_stat_RPC_CANTDECODERES)
        }))
        .err()
        .unwrap();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"decoder panic"));
    }

    #[test]
    fn nested_calls_are_rejected_and_scope_is_restored() {
        let outer = CallScope::enter().unwrap();
        assert!(matches!(
            call_string(c"unused", 1, 1, 1, c"abc", c"udp"),
            Err(Error::ReentrantCall),
        ));
        let callback = CallScope::callback();
        assert!(matches!(CallScope::enter(), Err(Error::ReentrantCall)));
        drop(callback);
        assert!(matches!(CallScope::enter(), Err(Error::ReentrantCall)));
        drop(outer);
        assert!(CallScope::enter().is_ok());
    }
}

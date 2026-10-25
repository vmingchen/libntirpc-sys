//! Synthetic RPC boundary benchmark: substitutes only runtime initialization
//! and the network call. Both paths execute real ntirpc XDR encode/decode/free.
//! Run separately from real network timing: bash tests/bench-rpc-string.sh.
use libntirpc_sys::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, c_void};
use std::hint::black_box;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

static COUNT: AtomicBool = AtomicBool::new(false);
static RUST_ALLOCS: AtomicUsize = AtomicUsize::new(0);
static NATIVE_ALLOCS: AtomicUsize = AtomicUsize::new(0);
static NATIVE_FREES: AtomicUsize = AtomicUsize::new(0);
static FREE: OnceLock<mem_free_size_t> = OnceLock::new();
static MALLOC: OnceLock<mem_1_size_t> = OnceLock::new();

struct Allocator;
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

// SAFETY: Each operation delegates unchanged allocations and layouts to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT.load(Relaxed) {
            RUST_ALLOCS.fetch_add(1, Relaxed);
        }
        // SAFETY: Forward the caller's valid layout to System.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Forward the caller's original System pointer and layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNT.load(Relaxed) {
            RUST_ALLOCS.fetch_add(1, Relaxed);
        }
        // SAFETY: Forward the caller's live allocation and requested size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

/// # Safety
/// Arguments must satisfy the saved ntirpc malloc hook's contract.
unsafe extern "C" fn count_malloc(
    size: usize,
    file: *const c_char,
    line: i32,
    func: *const c_char,
) -> *mut c_void {
    if COUNT.load(Relaxed) {
        NATIVE_ALLOCS.fetch_add(1, Relaxed);
    }
    // SAFETY: The original hook was saved before installation; forward its ABI.
    unsafe { MALLOC.get().unwrap().unwrap()(size, file, line, func) }
}

/// # Safety
/// Arguments must satisfy the saved ntirpc free hook's contract.
unsafe extern "C" fn count_free(
    pointer: *mut c_void,
    size: usize,
    file: *const c_char,
    line: i32,
    func: *const c_char,
) {
    if COUNT.load(Relaxed) && !pointer.is_null() {
        NATIVE_FREES.fetch_add(1, Relaxed);
    }
    // SAFETY: Ownership and the original free hook's ABI are forwarded unchanged.
    unsafe { FREE.get().unwrap().unwrap()(pointer, size, file, line, func) }
}

// SAFETY: This benchmark provides the exact C symbol/signature in svc.h, solely
// in this executable. No worker threads or transports are created in this mock.
#[unsafe(no_mangle)]
extern "C" fn __wrap_svc_init(_params: *mut svc_init_params) -> bool {
    true
}

/// # Safety
/// inproc/outproc must accept the provided argument slots and valid XDR streams.
/// All slots and borrowed storage must remain live through this synchronous call.
// SAFETY: This benchmark substitutes the exact rpc_call symbol/ABI from clnt.h.
#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_rpc_call(
    _host: *const c_char,
    _program: rpcprog_t,
    _version: rpcvers_t,
    procedure: rpcproc_t,
    inproc: xdrproc_t,
    input: *const c_void,
    outproc: xdrproc_t,
    output: *mut c_void,
    _transport: *const c_char,
) -> clnt_stat {
    if procedure == 3 {
        return clnt_stat_RPC_SUCCESS;
    }
    let mut bytes = [0u32; 16];
    // SAFETY: XDR contains only C scalars, pointers and nullable callbacks.
    let mut stream: XDR = unsafe { std::mem::zeroed() };
    // SAFETY: Aligned initialized bytes and the stream remain stable through
    // both callbacks; each callback is paired with its caller-supplied slot.
    unsafe {
        xdrmem_ncreate(
            &mut stream,
            bytes.as_mut_ptr().cast(),
            64,
            xdr_op_XDR_ENCODE,
        );
        if !inproc.unwrap()(&mut stream, input.cast_mut()) {
            return clnt_stat_RPC_CANTENCODEARGS;
        }
        xdrmem_ncreate(
            &mut stream,
            bytes.as_mut_ptr().cast(),
            64,
            xdr_op_XDR_DECODE,
        );
        if !outproc.unwrap()(&mut stream, output) {
            return clnt_stat_RPC_CANTDECODERES;
        }
    }
    if procedure == 2 {
        clnt_stat_RPC_FAILED
    } else {
        clnt_stat_RPC_SUCCESS
    }
}

/// # Safety
/// arg must point to a string-pointer slot appropriate to the stream's mode.
unsafe extern "C" fn wrap_string(stream: *mut XDR, arg: *mut c_void) -> bool {
    // SAFETY: The benchmark pairs this callback with its proper live slot.
    unsafe { xdr_wrapstring(stream, arg.cast()) }
}

fn raw_call() -> xdr::XdrString {
    let mut input = c"Alice".as_ptr().cast_mut();
    let mut output = std::ptr::null_mut::<c_char>();
    // SAFETY: Live C strings and pointer slots match wrap_string. Mock RPC
    // synchronously decodes an owned ntirpc allocation into the null output.
    unsafe {
        assert_eq!(
            __wrap_rpc_call(
                c"localhost".as_ptr(),
                1,
                1,
                1,
                Some(wrap_string),
                (&raw mut input).cast(),
                Some(wrap_string),
                (&raw mut output).cast(),
                c"udp".as_ptr()
            ),
            clnt_stat_RPC_SUCCESS
        );
        xdr::XdrString::from_raw(output).unwrap()
    }
}

fn safe_call() -> xdr::XdrString {
    client::call_string(c"localhost", 1, 1, 1, c"Alice", c"udp").unwrap()
}

fn measure(label: &str, operation: fn() -> xdr::XdrString) {
    for _ in 0..1000 {
        drop(black_box(operation()));
    }
    RUST_ALLOCS.store(0, Relaxed);
    NATIVE_ALLOCS.store(0, Relaxed);
    NATIVE_FREES.store(0, Relaxed);
    COUNT.store(true, Relaxed);
    let iterations = 200_000;
    let start = Instant::now();
    for _ in 0..iterations {
        drop(black_box(operation()));
    }
    let duration = start.elapsed();
    COUNT.store(false, Relaxed);
    let rust = RUST_ALLOCS.load(Relaxed);
    let native = NATIVE_ALLOCS.load(Relaxed);
    assert_eq!(
        NATIVE_FREES.load(Relaxed),
        iterations,
        "{label}: every result must be freed"
    );
    assert_eq!(rust, 0, "{label}: unexpected Rust allocations");
    assert_eq!(
        native, iterations,
        "{label}: expected one decoded string per call"
    );
    println!(
        "{label}: {:.1} ns/call; {rust} Rust allocations, {} native allocation/call",
        duration.as_nanos() as f64 / iterations as f64,
        native / iterations
    );
}

fn main() {
    if option_env!("LIBNTIRPC_SYS_RPC_BENCH_MOCK") != Some("1") {
        eprintln!("Run this isolated benchmark with bash tests/bench-rpc-string.sh");
        return;
    }
    // SAFETY: This executable starts no native workers (svc_init is mocked),
    // installs once before allocating XDR strings and retains this hook forever.
    unsafe {
        assert!(MALLOC.set(__ntirpc_pkg_params.malloc_).is_ok());
        assert!(FREE.set(__ntirpc_pkg_params.free_size_).is_ok());
        __ntirpc_pkg_params.malloc_ = Some(count_malloc);
        __ntirpc_pkg_params.free_size_ = Some(count_free);
    }
    // Verify cleanup when RPC reports failure after successfully decoding.
    COUNT.store(true, Relaxed);
    assert!(matches!(
        client::call_string(c"localhost", 1, 1, 2, c"Alice", c"udp"),
        Err(client::Error::CallFailed(status)) if status == clnt_stat_RPC_FAILED
    ));
    COUNT.store(false, Relaxed);
    assert_eq!(NATIVE_ALLOCS.load(Relaxed), 1);
    assert_eq!(NATIVE_FREES.load(Relaxed), 1);
    assert!(matches!(
        client::call_string(c"localhost", 1, 1, 3, c"Alice", c"udp"),
        Err(client::Error::MissingResult)
    ));
    measure("raw RPC boundary", raw_call);
    measure("safe RPC boundary", safe_call);
}

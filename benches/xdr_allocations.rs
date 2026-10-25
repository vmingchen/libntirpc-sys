//! Single-threaded allocation checks, isolated from tests and throughput timing.
mod support;

use libntirpc_sys::xdr::{MemoryDecoder, MemoryEncoder};
use libntirpc_sys::{__ntirpc_pkg_params, mem_1_size_t, mem_free_size_t};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CString, c_char, c_int, c_void};
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
};
use support::{owned_round_trip, reused_round_trip};

static ENABLED: AtomicBool = AtomicBool::new(false);
static RUST_CALLS: AtomicUsize = AtomicUsize::new(0);
static NATIVE_ALLOCS: AtomicUsize = AtomicUsize::new(0);
static NATIVE_FREES: AtomicUsize = AtomicUsize::new(0);

struct Counter;
#[global_allocator]
static ALLOCATOR: Counter = Counter;

// SAFETY: All operations delegate to System with unchanged pointers/layouts.
// Counters are atomic and do not allocate or access allocation contents.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ENABLED.load(Relaxed) {
            RUST_CALLS.fetch_add(1, Relaxed);
        }
        // SAFETY: Forward the caller's valid layout to System.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Forward the original System allocation and layout unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if ENABLED.load(Relaxed) {
            RUST_CALLS.fetch_add(1, Relaxed);
        }
        // SAFETY: Forward the caller's System allocation and valid sizes.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

struct Hooks {
    malloc: mem_1_size_t,
    free: mem_free_size_t,
}
static ORIGINAL: OnceLock<Hooks> = OnceLock::new();
struct NativeCounters;

impl NativeCounters {
    fn install() -> Self {
        // SAFETY: This standalone benchmark starts no ntirpc worker threads.
        // No XDR strings are live during installation; callbacks only count and
        // forward to the original allocator. ORIGINAL is set before publication.
        unsafe {
            let hooks = Hooks {
                malloc: __ntirpc_pkg_params.malloc_,
                free: __ntirpc_pkg_params.free_size_,
            };
            assert!(hooks.malloc.is_some() && hooks.free.is_some());
            assert!(ORIGINAL.set(hooks).is_ok());
            __ntirpc_pkg_params.malloc_ = Some(count_malloc);
            __ntirpc_pkg_params.free_size_ = Some(count_free);
        }
        Self
    }
}

impl Drop for NativeCounters {
    fn drop(&mut self) {
        let hooks = ORIGINAL.get().unwrap();
        // SAFETY: All measured operations/strings have finished, no native worker
        // threads exist, and these are the same allocator functions we replaced.
        unsafe {
            __ntirpc_pkg_params.malloc_ = hooks.malloc;
            __ntirpc_pkg_params.free_size_ = hooks.free;
        }
    }
}

/// Count an allocation while preserving ntirpc's allocator ABI.
///
/// # Safety
/// Arguments must meet the original malloc hook's contract. ORIGINAL must have
/// been installed before publishing this callback to the native library.
unsafe extern "C" fn count_malloc(
    size: usize,
    file: *const c_char,
    line: c_int,
    function: *const c_char,
) -> *mut c_void {
    if ENABLED.load(Relaxed) {
        NATIVE_ALLOCS.fetch_add(1, Relaxed);
    }
    // SAFETY: Installation saves the original hook; forward all C arguments.
    unsafe { (ORIGINAL.get().unwrap().malloc.unwrap())(size, file, line, function) }
}

/// Count a release through the matching original allocator.
///
/// # Safety
/// ptr/size and diagnostic arguments must meet the original free hook's contract.
unsafe extern "C" fn count_free(
    ptr: *mut c_void,
    size: usize,
    file: *const c_char,
    line: c_int,
    function: *const c_char,
) {
    if !ptr.is_null() && ENABLED.load(Relaxed) {
        NATIVE_FREES.fetch_add(1, Relaxed);
    }
    // SAFETY: Forward ownership to the original matching native free hook.
    unsafe { (ORIGINAL.get().unwrap().free.unwrap())(ptr, size, file, line, function) }
}

fn measure(operation: impl FnOnce()) -> (usize, usize, usize) {
    RUST_CALLS.store(0, Relaxed);
    NATIVE_ALLOCS.store(0, Relaxed);
    NATIVE_FREES.store(0, Relaxed);
    ENABLED.store(true, Relaxed);
    operation();
    ENABLED.store(false, Relaxed);
    (
        RUST_CALLS.load(Relaxed),
        NATIVE_ALLOCS.load(Relaxed),
        NATIVE_FREES.load(Relaxed),
    )
}

fn main() {
    let _hooks = NativeCounters::install();
    println!("bytes\tpath\tRust alloc/realloc\tntirpc alloc\tntirpc free");
    for length in [16, 1024, 8192] {
        let input = CString::new(vec![b'a'; length]).unwrap();
        let capacity = 4 + length.next_multiple_of(4);
        let mut encoder = MemoryEncoder::new(capacity).unwrap();
        let mut decoder = MemoryDecoder::new(&[]).unwrap();
        reused_round_trip(&input, &mut encoder, &mut decoder); // Reserve before measuring.
        let owned = measure(|| owned_round_trip(&input, capacity));
        let reused = measure(|| reused_round_trip(&input, &mut encoder, &mut decoder));
        assert!(
            owned.0 > 0,
            "owned path must exercise the Rust allocation counter"
        );
        assert_eq!(
            reused.0, 0,
            "steady-state reuse must not allocate Rust storage"
        );
        assert_eq!((owned.1, owned.2), (1, 1));
        assert_eq!((reused.1, reused.2), (1, 1));
        for (path, (rust, alloc, free)) in [("owned", owned), ("reused", reused)] {
            println!("{length}\t{path}\t{rust}\t{alloc}\t{free}");
        }
    }
}

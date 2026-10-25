//! A minimal RPC client that greets a hello server.
//!
//! Requires the `hello_server` example to be running and an `rpcbind`
//! daemon to be up so the client can look up the server's port. Run it as:
//!
//! ```sh
//! cargo run --example hello_client -- [name] [host]
//! ```

use std::ffi::CString;
use std::os::raw::{c_char, c_void};

use libntirpc_sys::*;

const HELLO_PROG: rpcprog_t = 0x3000_0001;
const HELLO_VERS: rpcvers_t = 1;
const HELLO_PROC: rpcproc_t = 1;

mod support;
use support::{svc_req_alloc, svc_req_free, wrap_string};

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "world".to_string());
    let host = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let name_c = CString::new(name).unwrap();
    let host_c = CString::new(host).unwrap();
    let nettype = c"udp";

    // SAFETY: svc_init_params contains C scalars, raw pointers and nullable
    // callbacks, all admitting zero. Required fields are set before svc_init.
    let mut params: svc_init_params = unsafe { std::mem::zeroed() };
    // ntirpc requires svc_init() before any RPC activity, even for
    // clients. SVC_INIT_EPOLL must be set or max_events stays 0 and
    // the event loop that processes replies breaks with EINVAL.
    // SVC_INIT_NOREG_XPRTS is required too: otherwise svc_dg_ncreatef
    // registers the reply socket with the event channel before ntirpc
    // has installed its rendezvous callback, and a fast reply (from
    // rpcbind on localhost) can crash in svc_dg_rendezvous().
    params.flags = (SVC_INIT_EPOLL | SVC_INIT_NOREG_XPRTS) as u_long;
    params.max_events = 16;
    params.alloc_cb = Some(svc_req_alloc);
    params.free_cb = Some(svc_req_free);
    // SAFETY: This is the process's first ntirpc initialization. params is
    // initialized, copied by C, and its callbacks have static code lifetimes
    // and use matching Box allocation/free. NOREG prevents early dispatch.
    if !unsafe { svc_init(&mut params) } {
        eprintln!("svc_init failed");
        std::process::exit(1);
    }

    // rpc_call() performs a one-shot synchronous RPC: send a name,
    // receive the greeting.
    let mut name_ptr: *mut c_char = name_c.as_ptr() as *mut c_char;
    let mut result: *mut c_char = std::ptr::null_mut();

    // SAFETY: Host/nettype are live C strings. Input is NUL-terminated and
    // only read in XDR_ENCODE despite C's mutable signature. Result starts
    // null, and both erased arguments point to writable string pointers of
    // the type wrap_string expects. rpc_call uses them synchronously.
    let stat = unsafe {
        rpc_call(
            host_c.as_ptr(),
            HELLO_PROG,
            HELLO_VERS,
            HELLO_PROC,
            Some(wrap_string),
            &mut name_ptr as *mut *mut c_char as *const c_void,
            Some(wrap_string),
            &mut result as *mut *mut c_char as *mut c_void,
            nettype.as_ptr(),
        )
    };

    if stat != clnt_stat_RPC_SUCCESS {
        eprintln!("RPC call failed with status {stat}");
        std::process::exit(1);
    }

    // SAFETY: The successful synchronous RPC returned a NUL-terminated ntirpc
    // allocation, owned exclusively here. XdrString takes over its deallocation.
    let reply = unsafe { xdr::XdrString::from_raw(result) }
        .expect("successful string decode returned NULL");
    println!("{}", reply.as_c_str().to_string_lossy());
}

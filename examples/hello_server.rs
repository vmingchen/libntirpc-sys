//! A minimal RPC server that greets callers.
//!
//! Requires an `rpcbind` daemon to be running so the server can register
//! its program number. Run it as:
//!
//! ```sh
//! cargo run --example hello_server
//! ```
//!
//! ntirpc does not wire up request handling for you: the server is expected
//! to install a `rendezvous_cb` on the listening transport (which hooks the
//! per-request `process_cb`) just like nfs-ganesha does. See the comments
//! below.

use std::ffi::CString;
use std::net::UdpSocket;
use std::os::fd::IntoRawFd;
use std::os::raw::{c_char, c_void};
use std::time::Duration;

use libntirpc_sys::*;

const HELLO_PROG: rpcprog_t = 0x3000_0001;
const HELLO_VERS: rpcvers_t = 1;

mod support;
use support::{free_string, svc_req_alloc, svc_req_free, wrap_string};

/// The dispatch registered with `svc_reg` is only used for bookkeeping;
/// ntirpc never calls it (requests go to `process_cb` instead).
extern "C" fn hello_dispatch(_req: *mut svc_req) {}

// SAFETY: This declaration matches the public bundled rpc/svc_rqst.h API;
// the example needs explicit registration after installing its callback.
unsafe extern "C" {
    /// Register a configured transport on an event channel (zero = default).
    ///
    /// # Safety
    /// ntirpc must be initialized and xprt must be live and exclusively owned
    /// during registration, with its dispatch callbacks already installed.
    fn svc_rqst_evchan_reg(channel: u32, xprt: *mut SVCXPRT, flags: u32) -> i32;
}

/// Called by ntirpc for every incoming call (via `process_cb`). The client
/// sends its name as a string and receives "Hello, <name>!" back.
///
/// # Safety
/// req must be an initialized, exclusively dispatched ntirpc request with live
/// transport/XDR storage and valid operations. Its reply callback must consume
/// stack-backed results synchronously without retaining their pointers.
unsafe extern "C" fn hello_process(req: *mut svc_req) -> xprt_stat {
    // SAFETY: ntirpc supplies the live request/transport/XDR under the callback
    // contract above. Authentication initializes rq_auth before any reply. The
    // result union arm is written before sending; decoding publishes an owned
    // NUL-terminated string on success. Reply strings and their pointer slots
    // stay live through the synchronous send and are never used after freeing.
    unsafe {
        // ntirpc does not authenticate requests itself; the server must call
        // svc_auth_authenticate() to set up req->rq_auth. Without it, rq_auth is
        // NULL and svc_sendreply() skips the auth wrap, which is what actually
        // encodes the result body (so the client would get an empty reply).
        let mut no_dispatch = false;
        let authentication = svc_auth_authenticate(req, &mut no_dispatch);
        if authentication != auth_stat_AUTH_OK {
            return svcerr_auth(req, authentication);
        }
        if no_dispatch {
            return xprt_stat_XPRT_IDLE;
        }
        if (*req).rq_msg.cb_prog != HELLO_PROG {
            return svcerr_noprog(req);
        }
        if (*req).rq_msg.cb_vers != HELLO_VERS {
            return svcerr_progvers(req, HELLO_VERS, HELLO_VERS);
        }
        match (*req).rq_msg.cb_proc {
            0 => {
                (*req).rq_msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.proc_ = Some(xdr_void);
                (*req).rq_msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.where_ = std::ptr::null_mut();
                return svc_sendreply(req);
            }
            1 => {}
            _ => return svcerr_noproc(req),
        }

        let mut name: *mut c_char = std::ptr::null_mut();

        if !xdr_wrapstring((*req).rq_xdrs, &mut name) {
            return svcerr_decode(req);
        }
        let name_c = std::ffi::CStr::from_ptr(name);
        let reply_c = CString::new(format!("Hello, {}!", name_c.to_string_lossy())).unwrap();
        free_string(&mut name);

        // Prepare the reply. svc_sendreply() encodes the result synchronously,
        // so pointing at the reply string on the stack is fine.
        let mut reply_ptr: *mut c_char = reply_c.as_ptr() as *mut c_char;
        (*req).rq_msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.proc_ = Some(wrap_string);
        (*req).rq_msg.ru.RM_rmb.ru.RP_ar.ru.AR_results.where_ =
            &mut reply_ptr as *mut *mut c_char as *mut c_void;

        svc_sendreply(req);
        xprt_stat_XPRT_IDLE
    }
}

/// Called by ntirpc when the first datagram arrives on the listening socket
/// (and a new per-peer transport has been allocated). Hook up the request
/// handler for that transport and process the buffered datagram.
///
/// # Safety
/// xprt must be the live, exclusively dispatched per-peer transport supplied by
/// ntirpc, with initialized xp_ops. Its receive operation must obey ntirpc's
/// transport lifetime rules and invoke process_cb with valid requests.
unsafe extern "C" fn udp_rendezvous(xprt: *mut SVCXPRT) -> xprt_stat {
    // SAFETY: The rendezvous callback has exclusive access to this new per-peer
    // transport, so installing a static process callback cannot race another
    // dispatcher. xp_ops and any non-null xp_recv belong to this live transport.
    unsafe {
        vfsi_libntirpc_set_process_cb(xprt, Some(hello_process));
        let recv = (*(*xprt).xp_ops).xp_recv;
        match recv {
            Some(f) => f(xprt),
            None => xprt_stat_XPRT_DIED,
        }
    }
}

fn main() {
    // Start the service's thread pool and request handlers.
    // SVC_INIT_EPOLL must be set or max_events stays 0 and the event
    // loop that handles incoming requests breaks with EINVAL. The
    // request alloc/free callbacks are also required.
    // SAFETY: svc_init_params contains C scalars, raw pointers and nullable
    // callbacks, all admitting zero. Required fields are initialized below.
    let mut params: svc_init_params = unsafe { std::mem::zeroed() };
    params.flags = SVC_INIT_EPOLL as u_long;
    params.max_events = 16;
    params.alloc_cb = Some(svc_req_alloc);
    params.free_cb = Some(svc_req_free);
    // SAFETY: This is the process's first ntirpc initialization, params is
    // copied by C, and both request callbacks remain callable for its lifetime.
    if !unsafe { svc_init(&mut params) } {
        eprintln!("svc_init failed");
        std::process::exit(1);
    }

    let socket = UdpSocket::bind(("0.0.0.0", 0)).expect("bind UDP socket");
    let fd = socket.into_raw_fd();

    // SAFETY: fd transfers the bound socket to ntirpc with CLOSE ownership.
    // NOREG prevents workers from dispatching before callback installation.
    let xprt =
        unsafe { svc_dg_ncreatef(fd, 0, 0, SVC_CREATE_FLAG_CLOSE | SVC_CREATE_FLAG_XPRT_NOREG) };
    if xprt.is_null() {
        eprintln!("svc_dg_ncreatef failed");
        std::process::exit(1);
    }

    // ntirpc does not install this itself: without a rendezvous callback
    // the first incoming datagram calls a NULL function pointer inside
    // svc_dg_rendezvous() and crashes.
    // SAFETY: The non-null transport is initialized and not yet registered;
    // no worker can access its callback while this thread installs it.
    unsafe { (*xprt).xp_dispatch.rendezvous_cb = Some(udp_rendezvous) };

    // Register the program with rpcbind so clients can find our port.
    // The dispatch argument is only used for bookkeeping; ntirpc routes
    // calls to process_cb, never to this function.
    let nettype = c"udp";
    // SAFETY: nettype is a static NUL-terminated network name.
    let nconf = unsafe { getnetconfigent(nettype.as_ptr()) };
    if nconf.is_null() {
        eprintln!("getnetconfigent failed");
        std::process::exit(1);
    }
    // rpcbind keeps the mapping of a killed server around, and it will not
    // let a new process overwrite it. Clear any stale entry first.
    // SAFETY: nconf is the live config returned above and xprt is initialized
    // and exclusively owned before event registration. hello_dispatch is
    // static code; neither call retains nconf past freenetconfigent.
    let registered = unsafe {
        rpcb_unset(HELLO_PROG, HELLO_VERS, nconf);
        let registered = svc_reg(xprt, HELLO_PROG, HELLO_VERS, Some(hello_dispatch), nconf);
        freenetconfigent(nconf);
        registered
    };
    if !registered {
        eprintln!("svc_reg failed (is rpcbind running?)");
        std::process::exit(1);
    }
    // SAFETY: Initialization and callback/service registration are complete.
    // This transfers dispatch access to workers; main never touches xprt again.
    if unsafe { svc_rqst_evchan_reg(0, xprt, 0) } != 0 {
        eprintln!("event channel registration failed");
        std::process::exit(1);
    }

    println!("hello server: prog={HELLO_PROG:#x} vers={HELLO_VERS} on udp");

    // The service runs on its own threads; just keep the process alive.
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
#[path = "hello_server_tests.rs"]
mod tests;

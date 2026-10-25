//! A minimal RPC client that greets a hello server.
//!
//! Requires the `hello_server` example to be running and an `rpcbind`
//! daemon to be up so the client can look up the server's port. Run it as:
//!
//! ```sh
//! cargo run --example hello_client -- [name] [host]
//! ```

use std::ffi::CString;

use libntirpc_sys::*;

const HELLO_PROG: rpcprog_t = 0x3000_0001;
const HELLO_VERS: rpcvers_t = 1;
const HELLO_PROC: rpcproc_t = 1;

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "world".to_string());
    let host = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let name_c = CString::new(name).unwrap();
    let host_c = CString::new(host).unwrap();
    let reply =
        match client::call_string(&host_c, HELLO_PROG, HELLO_VERS, HELLO_PROC, &name_c, c"udp") {
            Ok(reply) => reply,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
    println!("{}", reply.as_c_str().to_string_lossy());
}

//! A minimal RPC client that greets a hello server.
//!
//! Requires the `hello_server` example to be running and an `rpcbind`
//! daemon to be up so the client can look up the server's port. Run it as:
//!
//! ```sh
//! cargo run --example hello_client -- [name] [host]
//! ```

use std::ffi::{CStr, CString};

use libntirpc_sys::client::{Client, RpcCodec};
use libntirpc_sys::xdr::{self, XdrDecoder, XdrEncoder, XdrString};
use libntirpc_sys::*;

const HELLO_PROG: rpcprog_t = 0x3000_0001;
const HELLO_VERS: rpcvers_t = 1;
const HELLO_PROC: rpcproc_t = 1;

struct Hello;

impl RpcCodec for Hello {
    const NUMBER: rpcproc_t = HELLO_PROC;
    type Request = CStr;
    type Reply = XdrString;

    fn encode(xdr: &mut XdrEncoder<'_>, name: &CStr) -> Result<(), xdr::Error> {
        xdr.encode_string(name)
    }

    fn decode(xdr: &mut XdrDecoder<'_>) -> Result<XdrString, xdr::Error> {
        xdr.decode_string()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "world".to_string());
    let host = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let name_c = CString::new(name)?;
    let host_c = CString::new(host)?;
    let client = Client::new(&host_c, HELLO_PROG, HELLO_VERS, c"udp")?;
    let reply = client.call::<Hello>(&name_c)?;
    println!("{}", reply.as_c_str().to_string_lossy());
    Ok(())
}

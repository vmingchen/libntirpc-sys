// Bindgen's raw ABI declarations and helpers are generated, including the
// docs.rs snapshot. Keep generator-specific exceptions inside this module so
// they cannot suppress the safety checks on handwritten code or tests.
mod bindings {
    #![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
    #![allow(unknown_lints, unnecessary_transmutes)]
    #![allow(improper_ctypes, improper_ctypes_definitions)]
    #![allow(unsafe_op_in_unsafe_fn)]
    #![allow(clippy::missing_safety_doc, clippy::undocumented_unsafe_blocks)]
    #![allow(clippy::ptr_offset_with_cast, clippy::manual_div_ceil)]
    #![allow(suspicious_runtime_symbol_definitions)]

    use super::rpcblist;
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
pub use bindings::*;
#[allow(non_camel_case_types)]
pub type rpcblist = rp__list;

pub mod xdr;

#[cfg(feature = "fuzzing")]
pub mod fuzzing;

// SAFETY: These signatures match src/auth_helpers.c, which is compiled against
// the same bundled headers and feature definitions as the generated bindings.
unsafe extern "C" {
    /// Release an AUTH reference using libntirpc's `auth_destroy` macro.
    ///
    /// # Safety
    /// `auth` must be a live, initialized AUTH from this library, and the caller
    /// must own a reference that has not already been released. It must not be
    /// accessed after this call if that reference was the last one.
    pub fn vfsi_libntirpc_auth_destroy(auth: *mut AUTH);
    /// Set a server transport's process callback without exposing bindgen's
    /// version-dependent representation of the anonymous dispatch union.
    ///
    /// # Safety
    /// `xprt` must be a live initialized transport. The caller must serialize
    /// callback replacement with all transport users. The callback must obey
    /// ntirpc's request lifetime rules and remain callable until unregistered.
    pub fn vfsi_libntirpc_set_process_cb(xprt: *mut SVCXPRT, callback: svc_req_fun_t);
    /// `sizeof(SVCXPRT)` as the C shims see it. Used to assert that the
    /// generated bindings and the shims agree on the transport ABI.
    ///
    /// This function has no caller preconditions and is safe to call.
    pub safe fn vfsi_libntirpc_sizeof_svcxprt() -> usize;
    #[cfg(feature = "rpcsec-gss")]
    /// Create GSS authentication through the C shim.
    ///
    /// # Safety
    /// `client` must remain live throughout negotiation and the returned AUTH's
    /// lifetime. `service` must point to a readable NUL-terminated service name;
    /// `security` must point to an initialized `rpcsec_gss::RpcGssSec` with valid
    /// mechanism/credential handles that remain valid for the AUTH's lifetime.
    /// Serialize access to the client and handles as required by ntirpc/GSS;
    /// release a returned AUTH reference exactly once.
    pub fn vfsi_libntirpc_authgss_ncreate_default(
        client: *mut CLIENT,
        service: *mut ::std::os::raw::c_char,
        security: *mut ::std::os::raw::c_void,
    ) -> *mut AUTH;
}

/// ABI declarations for libntirpc's optional RPCSEC_GSS client support.
///
/// These definitions intentionally live outside the generated base bindings:
/// including `<rpc/auth_gss.h>` makes bindgen's output depend on the host's
/// complete GSS implementation.  The small public ABI below has been stable
/// across supported libntirpc releases.
#[cfg(feature = "rpcsec-gss")]
pub mod rpcsec_gss {
    use std::os::raw::{c_char, c_int, c_uint, c_void};

    use super::{AUTH, CLIENT};

    pub const RPCSEC_GSS_SVC_NONE: c_int = 1;
    pub const RPCSEC_GSS_SVC_INTEGRITY: c_int = 2;

    /// libntirpc's `struct rpc_gss_sec`.
    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    pub struct RpcGssSec {
        /// `GSS_C_NO_OID` selects the implementation's default mechanism.
        pub mech: *mut c_void,
        /// `GSS_C_QOP_DEFAULT` is zero.
        pub qop: c_uint,
        pub svc: c_int,
        /// `GSS_C_NO_CREDENTIAL` uses the process's default credential cache.
        pub cred: *mut c_void,
        pub req_flags: c_uint,
    }

    // SAFETY: RpcGssSec mirrors struct rpc_gss_sec in the bundled auth_gss.h;
    // pointer handles and integer fields use the C ABI's widths and alignment.
    unsafe extern "C" {
        /// Create client authentication for an RPCSEC_GSS service.
        ///
        /// # Safety
        /// `client` must be live for negotiation and the returned AUTH's lifetime.
        /// `service` must be a readable NUL-terminated string. `security` must be
        /// initialized, with valid GSS mechanism and credential handles (or their
        /// documented null sentinels) that remain valid for the AUTH's lifetime.
        /// Serialize access to the client/GSS handles as required by ntirpc/GSS,
        /// and release the returned AUTH reference once.
        pub fn authgss_ncreate_default(
            client: *mut CLIENT,
            service: *mut c_char,
            security: *mut RpcGssSec,
        ) -> *mut AUTH;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn memory_xdr_encodes_decodes_and_frees_a_string() {
        let mut encoder = xdr::MemoryEncoder::new(8).unwrap();
        encoder.encode_string(c"abc").unwrap();
        let bytes = encoder.as_bytes().to_vec();
        assert_eq!(bytes, [0, 0, 0, 3, b'a', b'b', b'c', 0]);
        let mut decoder = xdr::MemoryDecoder::new(&bytes).unwrap();
        let decoded = decoder.decode_string().unwrap();
        assert_eq!(decoded.as_c_str(), c"abc");
        drop(decoded); // Matching ntirpc deallocation is handled by the owner.
    }

    /// The bindgen invocation and the C shims must compile `SVCXPRT` with the
    /// same feature macros (notably `INET6`), otherwise every field of the
    /// private `rpc_dplx_rec` after the transport is misaligned.
    #[test]
    fn svcxprt_abi_matches_shims() {
        let bindgen = std::mem::size_of::<SVCXPRT>();
        let shims = vfsi_libntirpc_sizeof_svcxprt();
        assert_eq!(
            bindgen, shims,
            "bindgen and the C shims disagree on sizeof(SVCXPRT) \
             ({bindgen} vs {shims}); check the -D flags passed to both",
        );
    }
}

#[cfg(test)]
#[path = "../tests/xdr.rs"]
mod xdr_regressions;

#[cfg(test)]
#[path = "../build_support.rs"]
mod build_support;

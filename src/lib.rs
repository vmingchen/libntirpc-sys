#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
pub type rpcblist = rp__list;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_basic() {
        assert_eq!(
            unsafe {
                xdr_void(
                    std::ptr::null_mut::<rpc_xdr>(),
                    std::ptr::null_mut::<std::os::raw::c_void>(),
                )
            },
            true
        );
    }
}

#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    libntirpc_sys::fuzzing::xdr_state(data);
});

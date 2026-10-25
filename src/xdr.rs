//! Safe memory-backed XDR string encoding and decoding.
//!
//! Stream modes, aligned buffers, stable stream addresses, and decoded-string
//! ownership are private invariants. Network streams can use
//! [`XdrString::decode_from_raw`], but establishing a raw stream's validity still
//! requires an unsafe call. Strings have C string semantics, not guaranteed UTF-8.
//!
//! ```
//! use libntirpc_sys::xdr::{MemoryDecoder, MemoryEncoder};
//!
//! let mut encoder = MemoryEncoder::new(32)?;
//! encoder.encode_string(c"hello")?;
//! let mut decoder = MemoryDecoder::new(encoder.as_bytes())?;
//! let text = decoder.decode_string()?;
//! assert_eq!(text.as_c_str(), c"hello");
//! # Ok::<(), libntirpc_sys::xdr::Error>(())
//! ```

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char};
use std::ptr::NonNull;

use crate::{XDR, xdr_op, xdr_op_XDR_DECODE, xdr_op_XDR_ENCODE, xdr_op_XDR_FREE};

/// An XDR operation or memory-stream construction failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The byte length does not fit ntirpc's 32-bit stream length.
    LengthOverflow,
    /// A borrowed raw stream was not in decode mode.
    WrongMode,
    /// Malformed/truncated input, an oversized string, or insufficient output space.
    OperationFailed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::LengthOverflow => "XDR memory stream length exceeds u32::MAX",
            Self::WrongMode => "XDR string decoder requires a decode stream",
            Self::OperationFailed => "XDR string operation failed",
        })
    }
}

impl std::error::Error for Error {}

fn zeroed_stream() -> XDR {
    // SAFETY: XDR contains C integers, raw pointers and nullable callbacks, all
    // admitting zero. xdrmem_ncreate leaves fields such as x_flags, x_handy and
    // x_v.vio_type/length untouched, so initialize the whole descriptor once.
    // The string-free path only reads x_op.
    unsafe { std::mem::zeroed() }
}

// Owns both allocations: x_base points into the boxed XDR, x_data into buffer.
// The stream allocation must stay stable. Resizing the buffer requires resetting
// all C pointers before another operation. xdrmem_destroy is a no-op, so Rust
// can drop both directly.
struct MemoryStream {
    // C retains an interior self-pointer. Access through UnsafeCell/raw pointers
    // avoids creating fresh Rust &mut XDR borrows over that retained pointer.
    stream: Box<UnsafeCell<XDR>>,
    buffer: Vec<u32>,
    length: u32,
    operation: xdr_op,
}

impl MemoryStream {
    fn new(mut buffer: Vec<u32>, length: u32, operation: xdr_op) -> Self {
        // An empty stream still gets an aligned, non-dangling buffer pointer.
        if buffer.is_empty() {
            buffer.push(0);
        }
        let mut result = Self {
            stream: Box::new(UnsafeCell::new(zeroed_stream())),
            buffer,
            length,
            operation,
        };
        result.rewind();
        result
    }

    fn rewind(&mut self) {
        // SAFETY: Private constructors/refill provide enough initialized u32
        // storage for length bytes and only ENCODE/DECODE modes. Exclusive access
        // prevents C calls or borrowed output views during reset. Reinitializing
        // restores every pointer after any buffer reallocation; XDR's self-pointer
        // still targets the same stable allocation. Memory streams own no C state
        // requiring destruction before reinitialization. Fields the constructor
        // leaves untouched were initialized once and are not modified by our
        // string or RPC-message operations. Neither descriptor nor buffer needs
        // clearing on rewind.
        unsafe {
            crate::xdrmem_ncreate(
                self.stream.get(),
                self.buffer.as_mut_ptr().cast(),
                self.length,
                self.operation,
            );
        }
    }

    fn position(&self) -> usize {
        // C keeps x_data within the allocation (including its one-past-end
        // position). Address arithmetic does not create references or read bytes.
        // SAFETY: Construction initialized XDR in this stable allocation. No C
        // operation can be active during this shared borrow: all operations
        // require &mut self, and no raw pointer escapes the safe public API.
        let cursor = unsafe { (*self.stream.get()).x_data };
        cursor.addr() - self.buffer.as_ptr().addr()
    }

    fn as_raw(&mut self) -> *mut XDR {
        self.stream.get()
    }
}

/// An owned, aligned input buffer and an XDR stream fixed in decode mode.
///
/// Construction copies the input, so the source slice may be dropped afterward.
/// A failed decoding operation may advance the cursor; [`Self::rewind`] retries
/// from the beginning without copying. [`Self::refill`] reuses the allocation for
/// another input. No raw fields can be changed through safe methods.
pub struct MemoryDecoder(MemoryStream);

impl MemoryDecoder {
    /// Copy bytes into aligned storage; reject lengths exceeding `u32::MAX`.
    pub fn new(data: &[u8]) -> Result<Self, Error> {
        let length = u32::try_from(data.len()).map_err(|_| Error::LengthOverflow)?;
        let buffer = data
            .chunks(4)
            .map(|chunk| {
                let mut word = [0; 4];
                word[..chunk.len()].copy_from_slice(chunk);
                u32::from_ne_bytes(word)
            })
            .collect();
        Ok(Self(MemoryStream::new(buffer, length, xdr_op_XDR_DECODE)))
    }

    /// Decode into a new ntirpc-owned string, automatically freed on drop.
    ///
    /// This never reuses caller storage, so a peer cannot overrun a supplied
    /// string buffer. ntirpc applies `xdr_wrapstring`'s protocol length limit.
    pub fn decode_string(&mut self) -> Result<XdrString, Error> {
        // SAFETY: The private memory stream is initialized for decoding, owns
        // its stable backing allocations, and is exclusively borrowed here.
        unsafe { XdrString::decode_from_raw(self.0.as_raw()) }
    }

    /// Restart decoding the current input without allocating or copying it.
    /// Already decoded strings are independent allocations and remain valid.
    pub fn rewind(&mut self) {
        self.0.rewind();
    }

    /// Copy another input into the reusable aligned buffer and rewind.
    ///
    /// Allocates only if the input exceeds the retained buffer capacity. A
    /// shorter input keeps that capacity but cannot read stale trailing bytes.
    /// Length overflow leaves the decoder unchanged. Decoded strings remain valid.
    pub fn refill(&mut self, data: &[u8]) -> Result<(), Error> {
        let length = u32::try_from(data.len()).map_err(|_| Error::LengthOverflow)?;
        // Extend fully initialized words instead of zero-filling resized storage
        // and then overwriting it. clear retains capacity for subsequent inputs.
        self.0.buffer.clear();
        self.0.buffer.extend(data.chunks(4).map(|chunk| {
            let mut bytes = [0; 4];
            bytes[..chunk.len()].copy_from_slice(chunk);
            u32::from_ne_bytes(bytes)
        }));
        if self.0.buffer.is_empty() {
            self.0.buffer.push(0);
        }
        self.0.length = length;
        self.0.rewind();
        Ok(())
    }

    /// Number of bytes consumed, including any partial failed operation.
    pub fn position(&self) -> usize {
        self.0.position()
    }

    // Used only by the crate's raw RPC-message fuzz adapters. Dereferencing or
    // passing this pointer to C remains unsafe and must preserve our invariants.
    #[cfg(feature = "fuzzing")]
    pub(crate) fn as_raw(&mut self) -> *mut XDR {
        self.0.as_raw()
    }
}

/// An owned, aligned output buffer and an XDR stream fixed in encode mode.
///
/// Failures may leave a partial encoding in the buffer; discard that encoding
/// before sending it. Encoding borrows strings and never transfers ownership.
pub struct MemoryEncoder(MemoryStream);

impl MemoryEncoder {
    /// Allocate space for at most `capacity` encoded bytes.
    pub fn new(capacity: usize) -> Result<Self, Error> {
        let length = u32::try_from(capacity).map_err(|_| Error::LengthOverflow)?;
        let buffer = vec![0; capacity.div_ceil(4)];
        Ok(Self(MemoryStream::new(buffer, length, xdr_op_XDR_ENCODE)))
    }

    /// Encode a borrowed NUL-terminated string using `xdr_wrapstring`.
    pub fn encode_string(&mut self, string: &CStr) -> Result<(), Error> {
        let mut pointer = string.as_ptr().cast_mut();
        // SAFETY: The private stream is live, exclusively borrowed and fixed
        // in encode mode. C only reads this NUL-terminated string, despite its
        // mutable pointer signature, and retains neither the string nor its slot.
        if unsafe { crate::xdr_wrapstring(self.0.as_raw(), &mut pointer) } {
            Ok(())
        } else {
            Err(Error::OperationFailed)
        }
    }

    /// Start another encoding without allocating or clearing the buffer.
    /// Old output is no longer visible; only bytes written afterward are exposed.
    pub fn reset(&mut self) {
        self.0.rewind();
    }

    /// Number of bytes written, including any partial failed operation.
    pub fn position(&self) -> usize {
        self.0.position()
    }

    /// Borrow written bytes without allocation or copying.
    ///
    /// The view excludes unused capacity and padding beyond the written prefix.
    /// Rust prevents encoding or resetting while this view is still in use:
    ///
    /// ```compile_fail
    /// use libntirpc_sys::xdr::MemoryEncoder;
    /// let mut encoder = MemoryEncoder::new(32).unwrap();
    /// encoder.encode_string(c"hello").unwrap();
    /// let view = encoder.as_bytes();
    /// encoder.reset();
    /// println!("{view:?}");
    /// ```
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: Initialized u32 values have no padding or invalid byte values.
        // position is within the allocated buffer, and its non-dangling pointer
        // is valid even for an empty view. The returned borrow prevents all safe
        // operations that could mutate, reset, reallocate, or drop this buffer.
        unsafe { std::slice::from_raw_parts(self.0.buffer.as_ptr().cast(), self.position()) }
    }
}

/// A uniquely owned NUL-terminated string allocated by ntirpc.
///
/// Drop frees through ntirpc's allocator, never Rust's CString allocator. The
/// string may outlive its decoding stream. Non-UTF-8 bytes are preserved;
/// embedded NULs on the wire terminate the visible C string, as in ntirpc.
/// A borrowed view cannot outlive its owner:
///
/// ```compile_fail
/// use libntirpc_sys::xdr::MemoryDecoder;
/// let view = {
///     let mut decoder = MemoryDecoder::new(&[0, 0, 0, 0]).unwrap();
///     let owned = decoder.decode_string().unwrap();
///     owned.as_c_str()
/// };
/// println!("{view:?}"); // owned has already been freed
/// ```
pub struct XdrString(NonNull<c_char>);

impl XdrString {
    /// Borrow the decoded C string for this owner's lifetime.
    pub fn as_c_str(&self) -> &CStr {
        // SAFETY: Constructors establish a live NUL-terminated allocation that
        // stays exclusively owned until Drop; no safe API permits its mutation.
        unsafe { CStr::from_ptr(self.0.as_ptr()) }
    }

    /// Adopt an allocation returned by ntirpc, or return None for null.
    ///
    /// # Safety
    /// A non-null pointer must be a live NUL-terminated string allocated by this
    /// ntirpc instance. Transfer its unique ownership here: no other user may
    /// mutate, free, or retain a reference past this owner's lifetime. Keep the
    /// library's allocator hooks valid and unchanged until Drop.
    pub unsafe fn from_raw(pointer: *mut c_char) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    /// Decode a new owned string from a borrowed network or other raw stream.
    ///
    /// # Safety
    /// stream must be non-null and point to an initialized ntirpc XDR stream
    /// with valid callbacks and sufficient live backing storage for its declared
    /// bounds. Provide exclusive access for the synchronous call; callbacks must
    /// obey ntirpc's XDR contract. Do not replace allocator hooks while the
    /// returned string is live. The constructor checks DECODE mode before use.
    pub unsafe fn decode_from_raw(stream: *mut XDR) -> Result<Self, Error> {
        // SAFETY: The caller guarantees a live readable initialized XDR.
        if unsafe { (*stream).x_op } != xdr_op_XDR_DECODE {
            return Err(Error::WrongMode);
        }
        let mut pointer = std::ptr::null_mut();
        // SAFETY: The caller provides exclusive use of a valid decode stream;
        // the null output forces allocation instead of reusing unknown storage.
        // The bundled decoder frees unpublished allocations on failure.
        if !unsafe { crate::xdr_wrapstring(stream, &mut pointer) } {
            return Err(Error::OperationFailed);
        }
        // SAFETY: Successful decoding returns a uniquely owned NUL-terminated
        // ntirpc allocation; this wrapper takes over its eventual release.
        unsafe { Self::from_raw(pointer) }.ok_or(Error::OperationFailed)
    }
}

impl Drop for XdrString {
    fn drop(&mut self) {
        let mut freeing = zeroed_stream();
        freeing.x_op = xdr_op_XDR_FREE;
        let mut pointer = self.0.as_ptr();
        // SAFETY: This owner holds the unique live ntirpc allocation. XDR_FREE
        // only reads x_op, uses the matching allocator, and clears this slot.
        unsafe { crate::xdr_wrapstring(&mut freeing, &mut pointer) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_round_trip_and_outlive_their_stream() {
        let strings = [c"", c"abc", c"\xff\xfe"];
        let mut encoder = MemoryEncoder::new(64).unwrap();
        for string in strings {
            encoder.encode_string(string).unwrap();
        }
        assert_eq!(encoder.position(), 20);
        let bytes = encoder.as_bytes().to_vec();
        assert_eq!(&bytes[..4], &[0; 4]);
        let mut decoder = MemoryDecoder::new(&bytes).unwrap();
        drop(bytes);
        drop(encoder);
        let decoded: Vec<_> = strings
            .iter()
            .map(|_| decoder.decode_string().unwrap())
            .collect();
        assert_eq!(decoder.position(), 20);
        drop(decoder);
        for (value, expected) in decoded.iter().zip(strings) {
            assert_eq!(value.as_c_str(), expected);
        } // Each independent allocation is released here, after its stream.
    }

    #[test]
    fn arbitrary_input_alignment_and_stream_moves_are_safe() {
        let wire = [0xaa, 0, 0, 0, 3, b'a', b'b', b'c', 0];
        let decoder = MemoryDecoder::new(&wire[1..]).unwrap();
        let mut moved = Box::new(decoder);
        let raw = moved.0.stream.get();
        // SAFETY: The decoder exclusively owns this initialized allocation and
        // no C operation is active while inspecting its raw self-pointer.
        unsafe {
            assert_eq!((*raw).x_base.cast_const(), (&raw const (*raw).x_v).cast());
        }
        assert_eq!(moved.decode_string().unwrap().as_c_str(), c"abc");
    }

    #[test]
    fn malformed_strings_and_short_outputs_return_errors() {
        let wire = [0, 0, 0, 3, b'a', b'b', b'c', 0];
        for length in 0..wire.len() {
            let mut decoder = MemoryDecoder::new(&wire[..length]).unwrap();
            assert!(matches!(
                decoder.decode_string(),
                Err(Error::OperationFailed)
            ));
            assert!(decoder.position() <= length);
        }
        let mut oversized = MemoryDecoder::new(&u32::MAX.to_be_bytes()).unwrap();
        assert!(matches!(
            oversized.decode_string(),
            Err(Error::OperationFailed)
        ));
        for capacity in 0..8 {
            let mut encoder = MemoryEncoder::new(capacity).unwrap();
            assert_eq!(encoder.encode_string(c"abc"), Err(Error::OperationFailed));
            assert!(encoder.position() <= capacity);
            assert_eq!(encoder.as_bytes().len(), encoder.position());
        }
    }

    #[test]
    fn ntirpc_string_limit_is_respected_in_both_directions() {
        // RPC_MAXDATASIZE is 9000 in the bundled rpc_com.h.
        let max = std::ffi::CString::new(vec![b'a'; 9000]).unwrap();
        let mut encoder = MemoryEncoder::new(9012).unwrap();
        encoder.encode_string(&max).unwrap();
        let mut decoder = MemoryDecoder::new(encoder.as_bytes()).unwrap();
        assert_eq!(decoder.decode_string().unwrap().as_c_str(), max.as_c_str());
        let oversized = std::ffi::CString::new(vec![b'a'; 9001]).unwrap();
        assert_eq!(
            encoder.encode_string(&oversized),
            Err(Error::OperationFailed)
        );
        let mut decoder = MemoryDecoder::new(&9001u32.to_be_bytes()).unwrap();
        assert!(matches!(
            decoder.decode_string(),
            Err(Error::OperationFailed)
        ));
    }

    #[test]
    fn raw_decoder_checks_the_stream_mode_before_using_it() {
        let mut encoder = MemoryEncoder::new(8).unwrap();
        // SAFETY: This is a live exclusively owned memory stream with initialized
        // callbacks. The wrapper must reject its ENCODE mode without changing it.
        let result = unsafe { XdrString::decode_from_raw(encoder.0.as_raw()) };
        assert!(matches!(result, Err(Error::WrongMode)));
        encoder.encode_string(c"abc").unwrap();
    }

    #[test]
    fn encoder_reuse_borrows_output_and_hides_stale_bytes() {
        let mut encoder = MemoryEncoder::new(64).unwrap();
        let buffer = encoder.0.buffer.as_ptr();
        let descriptor = encoder.0.stream.get();
        encoder.encode_string(c"a longer first message").unwrap();
        assert_eq!(encoder.as_bytes().as_ptr(), buffer.cast());
        let saved = encoder.as_bytes().to_vec();
        encoder.reset();
        assert!(encoder.as_bytes().is_empty());
        encoder.encode_string(c"a").unwrap();
        assert_eq!(encoder.as_bytes(), &[0, 0, 0, 1, b'a', 0, 0, 0]);
        assert_eq!(encoder.0.buffer.as_ptr(), buffer);
        assert_eq!(encoder.0.stream.get(), descriptor);
        // An explicitly owned output copy survives reset and subsequent writes.
        assert_eq!(
            MemoryDecoder::new(&saved)
                .unwrap()
                .decode_string()
                .unwrap()
                .as_c_str(),
            c"a longer first message"
        );
    }

    #[test]
    fn encoder_reset_recovers_after_a_partial_failed_encoding() {
        let mut encoder = MemoryEncoder::new(8).unwrap();
        assert_eq!(
            encoder.encode_string(c"too long"),
            Err(Error::OperationFailed)
        );
        assert_eq!(encoder.position(), 4); // The length was written first.
        encoder.reset();
        encoder.encode_string(c"abc").unwrap();
        assert_eq!(encoder.as_bytes(), &[0, 0, 0, 3, b'a', b'b', b'c', 0]);
    }

    #[test]
    fn decoder_rewinds_and_refills_without_reallocating_when_capacity_suffices() {
        let wire = [0, 0, 0, 3, b'a', b'b', b'c', 0];
        let mut decoder = MemoryDecoder::new(&wire).unwrap();
        let buffer = decoder.0.buffer.as_ptr();
        let descriptor = decoder.0.stream.get();
        let first = decoder.decode_string().unwrap();
        decoder.rewind();
        assert_eq!(decoder.position(), 0);
        assert_eq!(
            decoder.decode_string().unwrap().as_c_str(),
            first.as_c_str()
        );

        // Refill with an incomplete message. Bytes from the old abc payload
        // must not satisfy this new message's missing payload or padding.
        decoder.refill(&[0, 0, 0, 3]).unwrap();
        assert!(matches!(
            decoder.decode_string(),
            Err(Error::OperationFailed)
        ));
        decoder.rewind();
        assert!(matches!(
            decoder.decode_string(),
            Err(Error::OperationFailed)
        ));
        decoder.refill(&[]).unwrap();
        assert_eq!(decoder.position(), 0);
        assert!(matches!(
            decoder.decode_string(),
            Err(Error::OperationFailed)
        ));
        decoder.refill(&wire).unwrap();
        assert_eq!(decoder.decode_string().unwrap().as_c_str(), c"abc");
        assert_eq!(decoder.0.buffer.as_ptr(), buffer);
        assert_eq!(decoder.0.stream.get(), descriptor);
        assert_eq!(first.as_c_str(), c"abc"); // Independent of every refill.
    }

    #[test]
    fn decoder_refill_repairs_pointers_after_growing_the_buffer() {
        let mut decoder = MemoryDecoder::new(&[]).unwrap();
        let descriptor = decoder.0.stream.get();
        let mut encoder = MemoryEncoder::new(1028).unwrap();
        let long = std::ffi::CString::new(vec![b'z'; 1024]).unwrap();
        encoder.encode_string(&long).unwrap();
        decoder.refill(encoder.as_bytes()).unwrap();
        let decoded = decoder.decode_string().unwrap();
        assert_eq!(decoded.as_c_str(), long.as_c_str());
        assert_eq!(decoder.0.stream.get(), descriptor);
        // Moving the owner and shrinking/refilling must preserve both buffers'
        // lifetime invariants and restore the exact logical input length.
        let mut moved = Box::new(decoder);
        moved.refill(&[0, 0, 0, 0]).unwrap();
        assert_eq!(moved.decode_string().unwrap().as_c_str(), c"");
        assert!(matches!(moved.decode_string(), Err(Error::OperationFailed)));
        assert_eq!(decoded.as_c_str(), long.as_c_str());
    }

    #[test]
    fn c_string_semantics_allow_non_utf8_and_stop_at_embedded_nul() {
        let mut decoder = MemoryDecoder::new(&[0, 0, 0, 3, b'a', 0, b'b', 0]).unwrap();
        let value = decoder.decode_string().unwrap();
        assert_eq!(value.as_c_str(), c"a");
        assert_eq!(decoder.position(), 8);
    }

    #[test]
    fn lengths_that_would_wrap_the_c_abi_are_rejected_without_allocation() {
        if usize::BITS > 32 {
            assert!(matches!(
                MemoryEncoder::new(usize::MAX),
                Err(Error::LengthOverflow)
            ));
        }
    }
}

//! Timing is kept separate from the allocation-counter binary.
mod support;

use libntirpc_sys::xdr::{MemoryDecoder, MemoryEncoder};
use std::ffi::CString;
use std::time::Instant;
use support::{owned_round_trip, reused_round_trip};

fn main() {
    let quick = std::env::args().any(|arg| arg == "--quick");
    println!("Payload bytes per round trip; median of 5 samples, alternating order.");
    println!("bytes\towned ns/op\treused ns/op\towned MiB/s\treused MiB/s\tspeedup");
    for (length, count) in [(16, 200_000), (1024, 100_000), (8192, 20_000)] {
        let count = if quick { count / 100 } else { count };
        let input = CString::new(vec![b'a'; length]).unwrap();
        let capacity = 4 + length.next_multiple_of(4);
        let mut encoder = MemoryEncoder::new(capacity).unwrap();
        let mut decoder = MemoryDecoder::new(&[]).unwrap();
        for _ in 0..100 {
            owned_round_trip(&input, capacity);
            reused_round_trip(&input, &mut encoder, &mut decoder);
        }
        let mut owned = Vec::new();
        let mut reused = Vec::new();
        for round in 0..5 {
            for reuse in [round % 2 == 0, round % 2 != 0] {
                let start = Instant::now();
                for _ in 0..count {
                    if reuse {
                        reused_round_trip(&input, &mut encoder, &mut decoder);
                    } else {
                        owned_round_trip(&input, capacity);
                    }
                }
                let ns = start.elapsed().as_nanos() as f64 / count as f64;
                if reuse {
                    reused.push(ns)
                } else {
                    owned.push(ns)
                }
            }
        }
        owned.sort_by(f64::total_cmp);
        reused.sort_by(f64::total_cmp);
        let mib_per_ns = length as f64 * 1e9 / (1024.0 * 1024.0);
        println!(
            "{length}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.2}x",
            owned[2],
            reused[2],
            mib_per_ns / owned[2],
            mib_per_ns / reused[2],
            owned[2] / reused[2]
        );
    }
}

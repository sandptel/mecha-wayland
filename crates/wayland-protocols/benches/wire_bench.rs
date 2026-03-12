use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::io::Cursor;
use wayland_protocols::wire;

// The pure CPU cost of decoding one Wayland wire-protocol message from a buffer already in memory
fn bench_parse_wire_message(c: &mut Criterion) {
    let mut body = Vec::new();

    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&6u32.to_le_bytes());
    body.extend_from_slice(b"hello\0\0\0");

    // Build the frame using the real send_msg from wire.rs.
    let mut frame = Vec::new();
    wire::send_msg(&mut frame, 1, 0, &body).unwrap();

    c.bench_function("event_loop_iteration: parse_wire_message", |b| {
        // Use the real recv_msg from wire.rs with a Cursor as the Read source.
        b.iter(|| wire::recv_msg(&mut Cursor::new(black_box(&frame))))
    });
}

// Benchmark: full display::sync → wl_callback::done roundtrip latency.
use std::env;
use std::io::Write;
use std::os::unix::net::UnixStream;

fn bench_sync_roundtrip(c: &mut Criterion) {
    let socket_path = {
        let rt = env::var("XDG_RUNTIME_DIR")
            .expect("XDG_RUNTIME_DIR not set — is a Wayland compositor running?");
        let disp = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        if disp.starts_with('/') {
            disp
        } else {
            format!("{}/{}", rt, disp)
        }
    };

    // One persistent connection across all samples — matches real app behaviour.
    let mut stream = UnixStream::connect(&socket_path)
        .expect("could not connect to Wayland compositor — is one running?");

    // Start object IDs above wl_display (1); each sync allocates one callback.
    let mut next_id: u32 = 2;

    c.bench_function("display::sync roundtrip", |b| {
        // iter_custom lets the benchmark own timing,
        // so only the latency of the IPC round-trip (not Criterion's own overhead) is counted
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iters {
                let callback_id = next_id;
                next_id += 1;

                // wl_display::sync — object_id=1, opcode=0, body=[new_id: u32]
                let mut frame = Vec::new();
                wire::send_msg(&mut frame, 1, 0, &callback_id.to_le_bytes()).unwrap();

                let t0 = std::time::Instant::now();
                stream.write_all(&frame).unwrap();

                // Drain events until wl_callback::done (opcode=0) for our id.
                // Other events (e.g. wl_display::delete_id) are skipped.
                loop {
                    let (obj_id, opcode, _body) = wire::recv_msg(&mut stream).unwrap();
                    if obj_id == callback_id && opcode == 0 {
                        break;
                    }
                }
                total += t0.elapsed();
            }
            total
        });
    });
}

criterion_group!(benches, bench_parse_wire_message, bench_sync_roundtrip);
criterion_main!(benches);

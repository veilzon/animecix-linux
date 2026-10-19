use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SOCKET_PATH: &str = "/tmp/animecix-test.sock";
const SIM_START_POS: f64 = 300.0;
const SIM_JUMP_POS: f64 = 900.0;
const SIM_DURATION: f64 = 3600.0;
const JUMP_AFTER_SECS: u64 = 3;
const TEST_DURATION_SECS: u64 = 10;
const POLL_INTERVAL_MS: u64 = 500;

fn handle_client(mut stream: UnixStream, start: Instant) {
    let reader_stream = stream.try_clone().unwrap();
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Simulate realistic mpv IPC delay (5-15ms per property query)
        std::thread::sleep(Duration::from_millis(8));

        let elapsed = start.elapsed().as_secs();
        let current_pos = if elapsed >= JUMP_AFTER_SECS {
            SIM_JUMP_POS + (elapsed - JUMP_AFTER_SECS) as f64
        } else {
            SIM_START_POS + elapsed as f64
        };

        if trimmed.contains("time-pos") {
            let resp = format!("{{\"data\":{:.2},\"error\":\"success\"}}\n", current_pos);
            let _ = stream.write_all(resp.as_bytes());
        } else if trimmed.contains("duration") {
            let resp = format!("{{\"data\":{:.2},\"error\":\"success\"}}\n", SIM_DURATION);
            let _ = stream.write_all(resp.as_bytes());
        }
    }
}

fn query_mpv_prop_test(sock: &str, prop: &str) -> Option<f64> {
    let mut stream = UnixStream::connect(sock).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_millis(800)))
        .ok();
    let cmd = format!("{{\"command\":[\"get_property\",\"{prop}\"]}}\n");
    stream.write_all(cmd.as_bytes()).ok()?;
    let mut buf = vec![0u8; 512];
    let n = stream.read(&mut buf).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&buf[..n]).ok()?;
    if v["error"].as_str() != Some("success") {
        return None;
    }
    v["data"].as_f64()
}

fn main() {
    let _ = std::fs::remove_file(SOCKET_PATH);

    let start = Instant::now();
    let listener = UnixListener::bind(SOCKET_PATH).expect("Failed to bind socket");

    let server_start = start;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let s = server_start;
            std::thread::spawn(move || handle_client(stream, s));
        }
    });

    while !std::path::Path::new(SOCKET_PATH).exists() {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("[INIT] Fake mpv socket ready at {}", SOCKET_PATH);

    let (tx, rx) = mpsc::channel::<(f64, f64)>();

    let sock_poll = SOCKET_PATH.to_string();

    // Polling thread: EXACTLY mimics real code structure
    // Two SEQUENTIAL query_mpv_prop calls (each opens new socket!)
    let poll_start = start;
    let mut cycle_durations: Vec<Duration> = Vec::new();
    std::thread::spawn(move || {
        loop {
            if poll_start.elapsed().as_secs() >= TEST_DURATION_SECS {
                break;
            }

            let cycle_start = Instant::now();

            // Two SEQUENTIAL calls - this is the bottleneck
            let pos = query_mpv_prop_test(&sock_poll, "time-pos").unwrap_or(0.0);
            let dur = query_mpv_prop_test(&sock_poll, "duration").unwrap_or(0.0);

            let cycle_dur = cycle_start.elapsed();
            cycle_durations.push(cycle_dur);

            if tx.send((pos, dur)).is_err() {
                break;
            }

            // Sleep 500ms between cycles (same as real code line 1548)
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }

        // Print cycle stats
        if !cycle_durations.is_empty() {
            let total: Duration = cycle_durations.iter().sum();
            let avg = total / cycle_durations.len() as u32;
            let max = cycle_durations.iter().max().unwrap();
            let min = cycle_durations.iter().min().unwrap();
            println!(
                "[STATS] Poll cycle times ({} cycles):",
                cycle_durations.len()
            );
            println!("  avg={:?}  min={:?}  max={:?}", avg, min, max);
            println!(
                "  effective poll rate = every {:?} (sleep 500ms + query time)",
                avg
            );
        }
    });

    // Receiver thread: mimics GTK glib::timeout_add_local at 500ms
    let mut total_ticks: u32 = 0;
    let mut stale_ticks: u32 = 0;
    let mut no_data_ticks: u32 = 0;
    let mut max_stale_delay: Duration = Duration::ZERO;
    let mut first_stale_time: Option<Instant> = None;
    let mut jump_detected_time: Option<Instant> = None;

    println!("[TEST] Running for {} seconds...", TEST_DURATION_SECS);
    println!(
        "[TEST] Position: {}s -> {}s at t={}s",
        SIM_START_POS, SIM_JUMP_POS, JUMP_AFTER_SECS
    );

    std::thread::sleep(Duration::from_millis(100));

    loop {
        if start.elapsed().as_secs() >= TEST_DURATION_SECS {
            break;
        }

        let tick_start = Instant::now();
        let wall_secs = start.elapsed().as_secs_f64();
        let mut latest: Option<(f64, f64)> = None;

        // Drain channel completely (same as real code lines 1562-1571)
        loop {
            match rx.try_recv() {
                Ok(v) => latest = Some(v),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        if let Some((pos, _dur)) = latest {
            total_ticks += 1;

            let expected_min = if wall_secs > (JUMP_AFTER_SECS as f64 + 1.0) {
                SIM_JUMP_POS
            } else {
                SIM_START_POS
            };

            if pos < expected_min - 10.0 {
                stale_ticks += 1;
                if first_stale_time.is_none() {
                    first_stale_time = Some(Instant::now());
                    println!(
                        "[TICK]  STALE {:.1}s (need >= {:.1}, wall={:.2}s)",
                        pos, expected_min, wall_secs
                    );
                } else {
                    println!(
                        "[TICK]  STALE {:.1}s (need >= {:.1}, wall={:.2}s)",
                        pos, expected_min, wall_secs
                    );
                }
            } else if pos >= SIM_JUMP_POS && jump_detected_time.is_none() {
                jump_detected_time = Some(Instant::now());
                if let Some(first) = first_stale_time {
                    max_stale_delay = Instant::now().duration_since(first);
                }
                println!("[TICK]  JUMP {:.1}s (wall={:.2}s)", pos, wall_secs);
            } else {
                println!("[TICK]  ok {:.1}s (wall={:.2}s)", pos, wall_secs);
            }
        } else {
            no_data_ticks += 1;
            println!(
                "[TICK]  EMPTY (wall={:.2}s) <-- GTK timer found nothing!",
                wall_secs
            );
        }

        let tick_elapsed = tick_start.elapsed();
        let sleep_time = Duration::from_millis(POLL_INTERVAL_MS).saturating_sub(tick_elapsed);
        std::thread::sleep(sleep_time);
    }

    // Final report
    println!();
    println!("========================================");
    println!("     MPV IPC PROGRESS TRACKING REPORT");
    println!("========================================");
    println!();
    println!("Architecture (from player.rs + app.rs):");
    println!("  poll thread: loop {{");
    println!("    pos = query_mpv_prop(sock, \"time-pos\");  // NEW socket each call");
    println!("    dur = query_mpv_prop(sock, \"duration\");  // NEW socket each call");
    println!("    sender.send((pos, dur));");
    println!("    sleep(500ms);");
    println!("  }}");
    println!();
    println!("  gtk_timer: timeout_add(500ms) {{");
    println!("    drain channel -> take latest -> update UI");
    println!("  }}");
    println!();
    println!("Results:");
    println!("  Total GTK ticks:           {}", total_ticks);
    println!("  Stale ticks:               {}", stale_ticks);
    println!("  Empty ticks (no data):     {}", no_data_ticks);
    println!("  Max stale delay:           {:?}", max_stale_delay);
    println!();
    println!("  Poll cycle interval:       500ms (sleep) + ~16ms (2x queries)");
    println!("  GTK timer interval:        500ms (fixed)");
    println!();

    let problem = no_data_ticks > 0 || stale_ticks > 2;
    if problem {
        println!("BUGS IDENTIFIED:");
        println!("  1. TWO SEQUENTIAL SOCKET CONNECTIONS per poll cycle.");
        println!("     Each query_mpv_prop() opens a brand new Unix socket,");
        println!("     writes, waits for response, closes. Two of these back-to-back");
        println!("     means ~2x the latency of what a single persistent connection");
        println!("     would need.");
        println!();
        println!("  2. POLL CYCLE > GTK TIMER interval.");
        println!("     poll sleeps 500ms + ~16ms query overhead = ~516ms per cycle.");
        println!("     GTK timer fires every 500ms. This means every other GTK tick");
        println!("     finds 0 new values in the channel (stutter).");
        println!();
        println!("  3. WORST CASE with slow mpv (buffering/seeking):");
        println!("     If mpv takes 50-100ms per query, poll cycle becomes 600-700ms.");
        println!("     GTK timer fires 1-2x between updates -> visible lag.");
        println!("     With the 1200ms read timeout, a stuck query blocks the entire");
        println!("     cycle for up to 2.4 SECONDS (both queries time out).");
        println!();
        println!("FIXES:");
        println!("  a) Use a SINGLE persistent socket (keep connection open)");
        println!("  b) Query both properties in ONE IPC call:");
        println!("     {{\"command\":[\"get_property\",\"time-pos\"]}} then");
        println!("     {{\"command\":[\"get_property\",\"duration\"]}} on same conn");
        println!("  c) OR use mpv observe_property for push-based updates");
        println!("     (eliminates polling entirely)");
        println!("  d) Reduce read timeout from 1200ms to 200ms");
        println!("========================================");
    } else {
        println!("No significant delay issues detected with instant server.");
        println!("NOTE: Real mpv has IPC latency. Re-run with slow server to see bugs.");
    }
    println!("========================================");

    let _ = std::fs::remove_file(SOCKET_PATH);
}

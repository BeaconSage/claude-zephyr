use crate::config::Config;
use crate::connection_tracker::ConnectionTracker;
use crate::events::ProxyEvent;
use crate::health_orchestrator::HealthCheckOrchestrator;
use crate::proxy::ProxyState;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Test timing synchronization between health check cycles and dashboard countdown
pub async fn test_health_check_timing() -> anyhow::Result<()> {
    println!("🧪 Starting health check timing self-test...");

    // Load config
    let config = Config::load_default()?;
    println!(
        "📋 Config loaded - base interval: {}s, min interval: {}s",
        config.health_check.interval_seconds,
        config.health_check.min_interval_seconds.unwrap_or(30)
    );

    // Create minimal test setup
    let connection_tracker = Arc::new(Mutex::new(ConnectionTracker::new()));
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel::<ProxyEvent>();
    let state = Arc::new(Mutex::new(ProxyState::new(config.clone())));

    // Start health check loop in background
    let health_state = state.clone();
    let health_config = config.clone();
    let health_sender = event_sender.clone();
    let health_tracker = connection_tracker.clone();

    tokio::spawn(async move {
        let (orchestrator, _command_sender) = HealthCheckOrchestrator::new(
            health_config,
            health_state,
            health_sender,
            false, // Enable console logs for testing
            Some(health_tracker),
        );
        let _ = orchestrator.run().await;
    });

    // Test timing for multiple cycles
    let mut test_results = Vec::new();
    let test_start = Instant::now();
    let test_duration = Duration::from_secs(120); // Test for 2 minutes

    let mut expected_next_check: Option<Instant> = None;
    let mut cycle_count = 0;
    let mut last_event_time = Instant::now();

    println!(
        "⏱️  Testing timing accuracy for {}s...",
        test_duration.as_secs()
    );
    println!("🔍 Looking for timing issues...\n");

    while test_start.elapsed() < test_duration {
        tokio::select! {
            event = event_receiver.recv() => {
                match event {
                    Some(ProxyEvent::HealthCheckStarted {
                        actual_interval,
                        next_check_time,
                        load_level,
                        active_connections
                    }) => {
                        cycle_count += 1;
                        let now = Instant::now();

                        println!("🔍 Cycle {}: interval={}s, load={:?}, conns={}",
                            cycle_count, actual_interval.as_secs(), load_level, active_connections);

                        // Check if this cycle started on time
                        if let Some(expected_time) = expected_next_check {
                            let timing_error = if now > expected_time {
                                now.duration_since(expected_time)
                            } else {
                                expected_time.duration_since(now)
                            };

                            let is_accurate = timing_error < Duration::from_secs(3); // 3s tolerance

                            test_results.push(TestResult {
                                cycle: cycle_count,
                                expected_time,
                                actual_time: now,
                                timing_error,
                                is_accurate,
                                interval: actual_interval,
                            });

                            println!("⏰ Timing error: {}ms ({})",
                                timing_error.as_millis(),
                                if is_accurate { "✅ OK" } else { "❌ FAILED" });
                        }

                        // Set expectation for next cycle
                        expected_next_check = Some(next_check_time);
                        last_event_time = now;

                        println!("📅 Next check expected at: {:?} (in {}s)\n",
                            next_check_time, actual_interval.as_secs());
                    },
                    _ => {
                        // Ignore other events
                    }
                }
            }

            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                // Check for stalled cycles
                if last_event_time.elapsed() > Duration::from_secs(80) {
                    println!("⚠️  WARNING: No health check events for {}s - possible stall!",
                        last_event_time.elapsed().as_secs());
                }
            }
        }
    }

    // Analyze results
    analyze_test_results(&test_results)?;

    Ok(())
}

#[derive(Debug)]
struct TestResult {
    cycle: u32,
    expected_time: Instant,
    actual_time: Instant,
    timing_error: Duration,
    is_accurate: bool,
    interval: Duration,
}

fn analyze_test_results(results: &[TestResult]) -> anyhow::Result<()> {
    println!("\n📊 Test Results Analysis:");
    println!("═══════════════════════");

    if results.is_empty() {
        println!("❌ CRITICAL FAILURE: No health check cycles detected!");
        println!(
            "   This indicates the health check loop is not running or events are not being sent."
        );
        return Err(anyhow::anyhow!(
            "No health check cycles observed during test"
        ));
    }

    let total_cycles = results.len();
    let accurate_cycles = results.iter().filter(|r| r.is_accurate).count();
    let accuracy_rate = (accurate_cycles as f64 / total_cycles as f64) * 100.0;

    let avg_error: Duration =
        results.iter().map(|r| r.timing_error).sum::<Duration>() / total_cycles as u32;

    let max_error = results
        .iter()
        .map(|r| r.timing_error)
        .max()
        .unwrap_or(Duration::ZERO);

    println!("Total cycles observed: {}", total_cycles);
    println!(
        "Accurate cycles: {}/{} ({:.1}%)",
        accurate_cycles, total_cycles, accuracy_rate
    );
    println!("Average timing error: {}ms", avg_error.as_millis());
    println!("Maximum timing error: {}ms", max_error.as_millis());

    // Show interval progression
    println!("\nInterval progression:");
    for (i, result) in results.iter().enumerate() {
        println!(
            "  Cycle {}: {}s interval, {}ms error",
            i + 1,
            result.interval.as_secs(),
            result.timing_error.as_millis()
        );
    }

    // Show detailed results for failed cycles
    let failed_cycles: Vec<_> = results.iter().filter(|r| !r.is_accurate).collect();
    if !failed_cycles.is_empty() {
        println!("\n❌ Failed cycles (>3000ms error):");
        for result in failed_cycles {
            println!(
                "  Cycle {}: {}ms error (interval: {}s)",
                result.cycle,
                result.timing_error.as_millis(),
                result.interval.as_secs()
            );
        }
    }

    // Determine overall test result
    let test_passed = accuracy_rate >= 70.0 && max_error < Duration::from_secs(10);

    println!("\n🏆 Overall Result:");
    if test_passed {
        println!("✅ PASSED - Timing synchronization is working correctly");
        Ok(())
    } else {
        println!("❌ FAILED - Timing synchronization issues detected");
        if accuracy_rate < 70.0 {
            println!(
                "   → Accuracy rate too low: {:.1}% (expected ≥70%)",
                accuracy_rate
            );
        }
        if max_error >= Duration::from_secs(10) {
            println!(
                "   → Maximum error too high: {}ms (expected <10000ms)",
                max_error.as_millis()
            );
        }

        println!("\n🔍 Possible causes:");
        println!("   • Health check execution taking longer than expected");
        println!("   • Network latency in health checks");
        println!("   • System load affecting timing");
        println!("   • Dashboard event processing delays");

        Err(anyhow::anyhow!("Timing synchronization test failed"))
    }
}

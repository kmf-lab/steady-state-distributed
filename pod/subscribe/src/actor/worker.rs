use steady_state::*;
use std::error::Error;
use std::time::Duration;

// === Constants for Channel and Batch Sizes ===

/// The total capacity of the generator and logger channels.
/// This is set high to support very large batch processing and high throughput.
const CHANNEL_CAPACITY: usize = 2_000_000;

/// The maximum number of values to a process per heartbeat (i.e., per batch).
/// This is the "batch size" for each processing cycle.
const VALUES_PER_HEARTBEAT: u64 = 1_000_000;


/// The output message type for the worker actor, representing the result of FizzBuzz logic.
///
/// This enum is used to encode the result of the FizzBuzz computation for each input value.
/// The `#[repr(u64)]` ensures that each variant is efficiently represented for channel transport.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
#[repr(u64)]
pub(crate) enum FizzBuzzMessage {
    #[default]
    FizzBuzz = 15, // For numbers divisible by 15
    Fizz = 3,      // For numbers divisible by 3 (but not 5)
    Buzz = 5,      // For numbers divisible by 5 (but not 3)
    Value(u64),    // For all other numbers
}

impl FizzBuzzMessage {
    /// Constructs a new FizzBuzzMessage from a given value.
    ///
    /// This function applies the FizzBuzz rules:
    /// - If the value is divisible by 15, returns FizzBuzz.
    /// - If divisible by 3 (but not 5), returns Fizz.
    /// - If divisible by 5 (but not 3), returns Buzz.
    /// - Otherwise, returns the value itself.
    pub fn new(value: u64) -> Self {
        match value % 15 {
            0 => FizzBuzzMessage::FizzBuzz,
            3 => FizzBuzzMessage::Fizz,
            5 => FizzBuzzMessage::Buzz,
            6 => FizzBuzzMessage::Fizz,
            9 => FizzBuzzMessage::Fizz,
            10 => FizzBuzzMessage::Buzz,
            12 => FizzBuzzMessage::Fizz,
            _ => FizzBuzzMessage::Value(value),
        }
    }
}

/// Persistent state for the worker actor.
///
/// This struct tracks all statistics and counters needed for robust, high-throughput processing.
/// By storing these values in persistent state, the worker can recover from panics or restarts
/// without losing track of progress or batch position.
pub(crate) struct WorkerState {
    /// Number of heartbeats (batches) processed.
    pub(crate) heartbeats_processed: u64,
    /// Number of input values processed.
    pub(crate) values_processed: u64,
    /// The next expected value for sequence validation.
    pub(crate) audit_position: u64,
}

/// Entry point for the worker actor.
///
/// This function is called by the actor system to start the worker. It receives:
/// - The actor context (`actor`), which manages the actor's lifecycle and provides access to arguments and control.
/// - An input channel (`heartbeat`) for receiving heartbeat signals (one per batch).
/// - An input channel (`generator`) for receiving the values to process.
/// - An output channel (`logger`) for sending FizzBuzzMessage results downstream.
/// - A persistent state object (`state`) for tracking all processing statistics.
///
/// The function links the actor to its input and output channels, then delegates to the core processing logic.
pub async fn run(
    actor: SteadyActorShadow,
    heartbeat: SteadyRx<u64>,
    generator: SteadyRx<u64>,
    logger: SteadyTx<FizzBuzzMessage>,
    state: SteadyState<WorkerState>,
) -> Result<(), Box<dyn Error>> {
    internal_behavior(
        actor.into_spotlight([&heartbeat, &generator], [&logger]),
        heartbeat,
        generator,
        logger,
        state,
    )
        .await
}

/// Core logic for the worker actor: high-throughput, robust batch processing.
///
/// This function implements the main loop for processing values in large batches, one per heartbeat.
/// It uses batching to maximize throughput, and it ensures that all state changes
/// (such as incrementing counters) are only made after successful operations.
/// The function is robust to failures: if the actor panics or crashes, the persistent
/// state ensures that it can resume without data loss or duplication.
///
/// The function also validates the sequence of input values, applies FizzBuzz logic,
/// and periodically logs statistics for monitoring and diagnostics.
async fn internal_behavior<A: SteadyActor>(
    mut actor: A,
    heartbeat: SteadyRx<u64>,
    generator: SteadyRx<u64>,
    logger: SteadyTx<FizzBuzzMessage>,
    state: SteadyState<WorkerState>,
) -> Result<(), Box<dyn Error>> {
    let args = actor.args::<crate::MainArg>().expect("unable to downcast");
    let beats = args.beats;
    let expected_total = beats * VALUES_PER_HEARTBEAT;


    // Lock the input and output channels for exclusive access during processing.
    let mut heartbeat = heartbeat.lock().await;
    let mut generator = generator.lock().await;
    let mut logger = logger.lock().await;

    // Initialize persistent state, ensuring batch_size matches VALUES_PER_HEARTBEAT.
    let mut state = state.lock(|| WorkerState {
        heartbeats_processed: 0,
        values_processed: 0,
        audit_position: 0,
    }).await;


    // Pre-allocate buffers for batch processing.
    let mut generator_batch = vec![0u64; VALUES_PER_HEARTBEAT as usize];
    let mut fizzbuzz_batch = Vec::with_capacity(VALUES_PER_HEARTBEAT as usize);

    // Main processing loop: runs until the actor system is shutting down and all channels are empty.
    while actor.is_running(|| {
        heartbeat.is_closed_and_empty() && generator.is_closed_and_empty() && logger.mark_closed()
    }) {
        // Wait for a heartbeat, at least one input value, and at least one output slot.
        await_for_all!(
            actor.wait_avail(&mut heartbeat, 1),
            actor.wait_avail(&mut generator, VALUES_PER_HEARTBEAT as usize),
            actor.wait_vacant(&mut logger, VALUES_PER_HEARTBEAT as usize)
        );

        // Consume one heartbeat (triggers a batch).
        let _ = actor.try_take(&mut heartbeat).unwrap();
        state.heartbeats_processed += 1;

        let taken = actor.take_slice(&mut generator, &mut generator_batch[..]).item_count();


        // Process the batch: apply FizzBuzz logic and validate sequence.
        fizzbuzz_batch.clear();
        for &value in &generator_batch[..taken] {
            if state.audit_position != value {
                panic!("Sequence error: expected {}, got {}", state.audit_position, value);
            }
            state.audit_position += 1;
            fizzbuzz_batch.push(FizzBuzzMessage::new(value));
        }
        // Ensure all taken items were processed.
        assert_eq!(taken, fizzbuzz_batch.len(), "Mismatch in processed items: taken {}, processed {}", taken, fizzbuzz_batch.len());

        // Send the processed batch to the logger.
        let sent_count = actor.send_slice(&mut logger, &fizzbuzz_batch).item_count();
        // Ensure all processed messages were sent.
        assert_eq!(taken, sent_count, "Failed to send all messages: processed {}, sent {}", taken, sent_count);

        state.values_processed += taken as u64;
        if state.values_processed >= expected_total {
            actor.request_shutdown().await;
        }

        // Log progress periodically for diagnostics.
        if 0 == (state.heartbeats_processed & ((1 << 26) - 1)) {
            trace!("Worker: {} heartbeats processed", state.heartbeats_processed);
        }
    }

    // Log shutdown statistics.
    info!(
        "Worker shutting down. Heartbeats: {}, Values: {}",
        state.heartbeats_processed,
        state.values_processed
    );

    Ok(())
}

/// Unit test for the worker actor's batch processing and FizzBuzz logic.
///
/// This test verifies that the worker can process multiple heartbeats, each with a large batch of values,
/// correctly apply FizzBuzz logic, and maintain sequence integrity. It uses a simulated actor system
/// and checks that the output matches the expected number of results.
#[cfg(test)]
pub(crate) mod worker_tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_worker() -> Result<(), Box<dyn Error>> {
        // Set up the test graph (actor system).
        let mut graph = GraphBuilder::for_testing().build(());
        let (generate_tx, generate_rx) = graph.channel_builder()
            .with_capacity(CHANNEL_CAPACITY)
            .build();
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder()
            .with_capacity(2)
            .build();
        let (logger_tx, logger_rx) = graph.channel_builder()
            .with_capacity(CHANNEL_CAPACITY)
            .build::<FizzBuzzMessage>();

        // Create shared state for the worker.
        let state = new_state();

        // Spawn the worker actor in the test graph.
        graph.actor_builder().with_name("UnitTest").build(
            move |context| internal_behavior(
                context,
                heartbeat_rx.clone(),
                generate_rx.clone(),
                logger_tx.clone(),
                state.clone()
            ),
            SoloAct,
        );

        // Test with 4 beats, processing up to 500,000 values per beat.
        let num_beats = 4;
        let total_values = num_beats * VALUES_PER_HEARTBEAT;
        let values: Vec<u64> = (0..total_values as u64).collect();
        generate_tx.testing_send_all(values, true);
        heartbeat_tx.testing_send_all((0..num_beats).map(|i| i as u64).collect(), true);

        // Run the graph.
        graph.start();
        sleep(Duration::from_millis(200)); // Allow time for processing
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;

        // Verify results: all values should be processed and sent.
        let results: Vec<FizzBuzzMessage> = logger_rx.testing_take_all();
        assert_eq!(results.len(), total_values, "Incorrect number of values processed");
        Ok(())
    }
}
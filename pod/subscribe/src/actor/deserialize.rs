use std::error::Error;
use steady_state::*;
use steady_state::distributed::distributed_stream::StreamControlItem; // TODO: make pub

/// Maintains the persistent state for the deserialization actor.
///
/// This struct is used to track the progress and correctness of the deserialization process.
/// It ensures that the actor can recover from panics or restarts without losing track of
/// shutdown signals or the expected sequence of numbers in each stream.
pub(crate) struct DeserializeState {
    /// Number of shutdown signals received (triggers shutdown when it reaches 2).
    shutdown_count: i32,
    /// Next expected number in the heartbeat stream to verify sequence.
    next_heartbeat: u64,
    /// Next expected number in the generator stream to verify sequence.
    next_generator: u64,
}

/// Entry point for the deserialization actor.
///
/// This function is called by the actor system to start the deserialization process. It receives:
/// - The actor context (`actor`), which manages the actor's lifecycle and provides access to arguments and control.
/// - An input bundle (`input`), which contains two stream channels (heartbeat and generator), each with a control and payload channel.
/// - Two output channels (`heartbeat`, `generator`) for sending deserialized u64 values downstream.
/// - A persistent state object (`state`) for tracking shutdown and sequence.
///
/// The function links the actor to its input and output channels, then delegates to the core processing logic.
pub(crate) async fn run(
    actor: SteadyActorShadow,
    input: SteadyStreamRxBundle<StreamIngress, 2>,
    heartbeat: SteadyTx<u64>,
    generator: SteadyTx<u64>,
    state: SteadyState<DeserializeState>,
) -> Result<(), Box<dyn Error>> {
    // Prepare the actor with metadata and output channels for processing.
    let actor = actor.into_spotlight(input.payload_meta_data(), [&heartbeat, &generator]);
    internal_behavior(actor, input, heartbeat, generator, state).await
}

/// Core logic for processing incoming streams, deserializing data, and forwarding it to output channels.
///
/// This function runs an infinite loop (until shutdown) that listens to two input streams (heartbeat and generator),
/// deserializes their 8-byte payloads into 64-bit unsigned integers, checks their sequence, and sends them to the
/// appropriate output channels. It handles shutdown signals and includes additional validation to ensure no
/// sequence errors occur in the outgoing data.
///
/// # Stream Structure
/// Each input stream consists of two channels:
///   - The control channel: for each group, a struct is read containing the length (in bytes)
///     of the corresponding payload.
///   - The payload channel: the actual serialized bytes, packed tightly.
///
/// The function ensures that the control and payload channels remain in sync: for every
/// control message, there is a corresponding block of bytes in the payload channel.
/// This is essential for correct deserialization and for maintaining data integrity.
///
/// # Sequence Checking
/// The function checks that the numbers in each stream arrive in strict order, panicking
/// if any out-of-order or missing value is detected. This is crucial for distributed
/// systems where data loss or duplication can occur.
async fn internal_behavior<A: SteadyActor>(
    mut actor: A,
    input: SteadyStreamRxBundle<StreamIngress, 2>,
    heartbeat: SteadyTx<u64>,
    generator: SteadyTx<u64>,
    state: SteadyState<DeserializeState>,
) -> Result<(), Box<dyn Error>> {
    // Gain exclusive access to the input streams.
    let mut input = input.lock().await;
    let mut rx_generator = input.remove(1); // Generator stream (index 1).
    let mut rx_heartbeat = input.remove(0); // Heartbeat stream (index 0).
    drop(input); // Release the input bundle to prevent unintended reuse.

    // Secure access to the output channels for sending deserialized data.
    let mut tx_heartbeat = heartbeat.lock().await;
    let mut tx_generator = generator.lock().await;

    // Initialize or access the shared state, setting defaults if first use.
    let mut state = state.lock(|| DeserializeState {
        shutdown_count: 0,
        next_heartbeat: 0,
        next_generator: 0,
    }).await;

    // Batch size is half the generator channel capacity for efficient processing.
    let batch_size = tx_generator.capacity() / 2;

    // Allocate buffers for efficient batch processing of incoming data.
    // - control_batch: stores the control messages (lengths) for each group.
    // - payload_batch: stores the raw bytes for each group.
    // - output_batch: stores the deserialized u64 values to be sent downstream.
    let mut control_batch = vec![StreamIngress::default(); batch_size];
    let mut payload_batch = vec![0u8; batch_size * 8];
    let mut output_batch = vec![0u64; batch_size];

    // Main processing loop: runs until the actor shuts down or streams are exhausted.
    while actor.is_running(|| {
        rx_heartbeat.is_empty()
            && rx_generator.is_empty()
            && tx_generator.mark_closed()
            && tx_heartbeat.mark_closed()
    }) {
        // Wait for data availability in either stream and space in output channels.
        // This uses "await_for_any" to allow either stream to be processed as soon as it is ready.
        await_for_any!(
            wait_for_all!(
                actor.wait_avail(&mut rx_heartbeat, 1),
                actor.wait_vacant(&mut tx_heartbeat, 1)
            ),
            wait_for_all!(
                actor.wait_avail(&mut rx_generator, 1),
                actor.wait_vacant(&mut tx_generator, 1)
            )
        );
        // trace!("top inputs heartbeats {:?} generator {:?}", rx_heartbeat.avail_units(), rx_generator.avail_units());
        // trace!("top outputs heartbeats {:?} generator {:?}", tx_heartbeat.vacant_units(), tx_generator.vacant_units());

        // === Process Heartbeat Stream ===
        // Calculate the maximum number of bytes to process in this batch, based on available output space.
        let bytes_limit = 8 * (batch_size.min(actor.vacant_units(&mut tx_heartbeat)));
        // Take a batch of control and payload data from the heartbeat stream.
        let done = actor.take_slice(&mut rx_heartbeat, (&mut control_batch[0..], &mut payload_batch[0..bytes_limit]));

        let mut byte_pos = 0;
        let mut output_idx = 0;
        // For each control message in the batch, process its payload.
        for i in 0..done.item_count() {
            let mut len = control_batch[i].length(); // Length in bytes of this message's payload.
            while len > 0 {
                // Extract the next 8 bytes and convert to u64.
                let slice = &payload_batch[byte_pos..(byte_pos + 8)];
                let beat = u64::from_be_bytes(slice.try_into().expect("Failed to convert 8 bytes to u64"));

                if beat == u64::MAX {
                    // Handle shutdown signal: increment counter and shutdown after 2 signals.
                    state.shutdown_count += 1;
                    if state.shutdown_count == 2 {
                        actor.request_shutdown().await;
                    }
                } else {
                    // Verify sequence integrity; panic if out of order.
                    if beat != state.next_heartbeat {
                        panic!("Heartbeat sequence error: expected {}, got {}", state.next_heartbeat, beat);
                    }
                    state.next_heartbeat += 1;
                    output_batch[output_idx] = beat;
                    output_idx += 1;
                }
                byte_pos += 8;
                len -= 8;
            }
        }
        if output_idx > 0 {
            // Extra validation: ensure the batch is consecutive.
            if output_idx > 1 {
                for i in 1..output_idx {
                    if output_batch[i] != output_batch[i - 1] + 1 {
                        panic!("Heartbeat batch not consecutive at index {}: {} followed by {}", i, output_batch[i - 1], output_batch[i]);
                    }
                }
            }
            // Log the batch details for debugging and traceability.
            trace!("Sending heartbeat batch: start={}, end={}, count={}", output_batch[0], output_batch[output_idx - 1], output_idx);
            actor.send_slice(&mut tx_heartbeat, &output_batch[0..output_idx]);
        }

        // === Process Generator Stream ===
        // Calculate the maximum number of bytes to process in this batch, based on available output space.
        let mut bytes_limit = 8 * (batch_size.min(actor.vacant_units(&mut tx_generator)));
        // Take a batch of control and payload data from the generator stream.
        let done = actor.take_slice(&mut rx_generator, (&mut control_batch[0..], &mut payload_batch[0..bytes_limit]));

        let mut byte_pos = 0;
        let mut output_idx = 0;
        // For each control message in the batch, process its payload.
        for i in 0..done.item_count() {
            let mut len = control_batch[i].length(); // Length in bytes of this message's payload.
            while len > 0 {
                // Extract the next 8 bytes and convert to u64.
                let slice = &payload_batch[byte_pos..(byte_pos + 8)];
                let beat = u64::from_be_bytes(slice.try_into().expect("Failed to convert 8 bytes to u64"));

                if beat == u64::MAX {
                    // Handle shutdown signal: increment counter and shutdown after 2 signals.
                    state.shutdown_count += 1;
                    if state.shutdown_count == 2 {
                        actor.request_shutdown().await;
                    }
                } else {
                    // Verify sequence integrity; panic if out of order.
                    if beat != state.next_generator {
                        panic!("Generator sequence error: expected {}, got {}", state.next_generator, beat);
                    }
                    state.next_generator += 1;
                    output_batch[output_idx] = beat;
                    output_idx += 1;
                }
                byte_pos += 8;
                len -= 8;
            }
        }
        if output_idx > 0 {
            // Extra validation: ensure the batch is consecutive.
            if output_idx > 1 {
                for i in 1..output_idx {
                    if output_batch[i] != output_batch[i - 1] + 1 {
                        panic!("Generator batch not consecutive at index {}: {} followed by {}", i, output_batch[i - 1], output_batch[i]);
                    }
                }
            }
            // Log the batch details for debugging and traceability.
            // trace!("Sending generator batch: start={}, end={}, count={}", output_batch[0], output_batch[output_idx - 1], output_idx);

            let items = actor.send_slice(&mut tx_generator, &output_batch[..output_idx]).item_count();
            assert_eq!(items, output_idx,"did not send all data");
        }
    }

    Ok(())
}

/// Unit tests for the deserialization logic.
///
/// These tests verify that the deserialization process correctly handles incoming data,
/// maintains sequence order, and shuts down appropriately when required.
#[cfg(test)]
pub(crate) mod deserialize_tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;
    use crate::arg::MainArg;

    /// Tests the basic deserialization and sequence checking functionality.
    ///
    /// This test sends two batches of sequential numbers to each input stream and verifies
    /// that the deserialized output matches the expected sequence. It also checks that
    /// shutdown and sequence validation work as intended.
    #[test]
    fn test_deserialize() -> Result<(), Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(MainArg::default());

        // Set up communication channels for testing.
        let (stream_tx, stream_rx) = graph.channel_builder().build_stream_bundle::<_, 2>(8);
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
        let (generator_tx, generator_rx) = graph.channel_builder().build();

        let state = new_state();
        graph
            .actor_builder()
            .with_name("UnitTest")
            .build(
                move |context| {
                    internal_behavior(
                        context,
                        stream_rx.clone(),
                        heartbeat_tx.clone(),
                        generator_tx.clone(),
                        state.clone(),
                    )
                },
                SoloAct,
            );

        // Utility function to serialize a sequence of numbers into a byte vector.
        fn serialize_numbers(nums: &[u64]) -> Vec<u8> {
            let mut bytes = Vec::with_capacity(nums.len() * 8);
            for &num in nums {
                bytes.extend_from_slice(&num.to_be_bytes());
            }
            bytes
        }

        let now = Instant::now();
        // Send sequential numbers to heartbeat stream: [0, 1] and [2, 3].
        let heartbeat_payload1 = serialize_numbers(&[0, 1]);
        let heartbeat_payload2 = serialize_numbers(&[2, 3]);
        stream_tx[0].testing_send_all(vec![
            StreamIngress::by_ref(16, now, now, &heartbeat_payload1),
            StreamIngress::by_ref(16, now, now, &heartbeat_payload2),
        ], true);

        // Send sequential numbers to generator stream: [0, 1] and [2, 3].
        let generator_payload1 = serialize_numbers(&[0, 1]);
        let generator_payload2 = serialize_numbers(&[2, 3]);
        stream_tx[1].testing_send_all(vec![
            StreamIngress::by_ref(16, now, now, &generator_payload1),
            StreamIngress::by_ref(16, now, now, &generator_payload2),
        ], true);

        // Start the graph, allow processing, then shut down.
        graph.start();
        sleep(Duration::from_millis(100));
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;

        // Verify that all numbers were received in order.
        let heartbeat_received: Vec<u64> = heartbeat_rx.testing_take_all();
        assert_eq!(heartbeat_received, vec![0, 1, 2, 3], "Heartbeat stream did not receive expected sequence");

        let generator_received: Vec<u64> = generator_rx.testing_take_all();
        assert_eq!(generator_received, vec![0, 1, 2, 3], "Generator stream did not receive expected sequence");

        Ok(())
    }
}
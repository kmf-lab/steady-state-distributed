use std::env;
use steady_state::*;
use arg::MainArg;
mod arg;

pub(crate) mod actor {
    pub(crate) mod deserialize;
    pub(crate) mod worker;
    pub(crate) mod logger;
}

// === ACTOR NAME CONSTANTS ===
const NAME_AERON: &str = "AERON";
const NAME_DESERIALIZE: &str = "DESERIALIZE";
const NAME_WORKER: &str = "WORKER";
const NAME_LOGGER: &str = "LOGGER";

///
/// # Distributed Subscriber Main
///
/// This is the entry point for the **Subscriber** side of a distributed, robust, high-throughput
/// actor pipeline. The subscriber is responsible for receiving data from Aeron, deserializing it,
/// processing it (FizzBuzz), and logging the results.
///
/// ## System Overview
///
/// The subscriber graph is composed of several robust actors, each running in its own thread,
/// connected by high-capacity channels. The pipeline is designed for maximum throughput,
/// reliability, and observability.
///
///
/// ```text
///   [AERON]
///      |
/// [STREAM BUNDLE]
///      |
/// [DESERIALIZE]
///   /        \
/// [HEARTBEAT] [GENERATOR]
///    |             |
///    +------[WORKER]------+
///               |
///            [LOGGER]
/// ```
///
/// - **AERON**: Receives dual-channel (control + payload) stream from the publisher.
/// - **STREAM BUNDLE**: Two channels per stream: one for control (lengths), one for payload (bytes).
/// - **DESERIALIZE**: Converts bytes back into u64 values, checks sequence, and splits into heartbeat and generator channels.
/// - **WORKER**: Consumes heartbeat and generator values, applies FizzBuzz logic, and produces FizzBuzzMessage results.
/// - **LOGGER**: Consumes FizzBuzzMessage results and logs statistics.
///
/// ## Key Features
///
/// - **High Throughput**: Channels and batches are sized for millions of messages per second.
/// - **Robustness**: Each actor is isolated and can recover from panics or failures.
/// - **Observability**: Telemetry, logging, and channel triggers provide deep insight into system health.
/// - **Distributed**: Input is streamed over Aeron, a high-performance messaging system.
///
/// ## Shutdown Barrier
///
/// The `.with_shutdown_barrier(2)` call ensures that the system will not fully shut down
/// until two independent shutdown signals are received. This is critical in distributed
/// systems to ensure that all data is flushed and all actors have completed their work
/// before the process exits. For example, one shutdown may come from the local graph,
/// and another from a remote publisher or a control plane.
///
/// ## Environment Setup
///
/// The telemetry server is configured to run on localhost:5552 for monitoring.
///
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up telemetry server environment variables for observability.
    unsafe {
        env::set_var("TELEMETRY_SERVER_PORT", "5552");
        env::set_var("TELEMETRY_SERVER_IP", "127.0.0.1");
    }

    // Parse command-line arguments (rate, beats, etc.) using clap.
    let cli_args = MainArg::parse();
    let _ = init_logging(LogLevel::Info);

    // Build the actor graph with all channels and actors, using the parsed arguments.
    // The shutdown barrier ensures the system waits for two shutdown signals before exiting.
    let mut graph = GraphBuilder::default()
        .build(cli_args);

    build_graph(&mut graph);

    // Start the entire actor system. All actors and channels are now live.
    graph.start();

    // The system runs until an actor requests shutdown or the timeout is reached.
    graph.block_until_stopped(std::time::Duration::from_secs(600))
}

///
/// # Graph Construction
///
/// This function builds the full actor pipeline and connects all channels.
/// It demonstrates the robust architecture:
/// - Each actor is built with persistent state, enabling automatic restart and state recovery.
/// - Channels are created for each stage of the pipeline, with triggers for observability.
/// - Each actor is built as a SoloAct, running on its own thread for failure isolation.
/// - The input is streamed in over Aeron using a dual-channel (control + payload) bundle.
///
/// ## Channel Triggers and Telemetry
///
/// The channel builder is configured with triggers that monitor buffer fill levels:
/// - **Red Alert**: If the channel is >90% full, a red alert is triggered.
/// - **Orange Alert**: If the channel is >60% full, an orange alert is triggered.
/// - **Average Fill/Rate**: Telemetry is collected for average fill and message rate.
///
/// These triggers are essential for diagnosing backpressure and tuning performance.
///
fn build_graph(graph: &mut Graph) {
    // Configure channel builders with telemetry triggers for observability.
    let channel_builder_base = graph.channel_builder()
        .with_filled_trigger(Trigger::AvgAbove(Filled::p90()), AlertColor::Red)
        .with_filled_trigger(Trigger::AvgAbove(Filled::p60()), AlertColor::Orange)
        .with_avg_filled()
        .with_avg_rate();

    // Small channel for heartbeat (low volume, high importance).
    let channel_builder_small = channel_builder_base.with_capacity(10_001);
    // Large channels for generator and stream output (high volume).
    let channel_builder_large = channel_builder_base.with_capacity(2_000_000);

    // Build channels for each stage of the pipeline.
    let (heartbeat_tx, heartbeat_rx) = channel_builder_small
        .with_labels(&["heartbeat"], true)
        .build_channel();
    let (generator_tx, generator_rx) = channel_builder_large
        .with_labels(&["generator"], true)
        .build_channel();
    let (worker_tx, worker_rx) = channel_builder_large.build_channel();

    // Build a dual-channel stream bundle for Aeron input.
    // Each stream has a control channel (for lengths) and a payload channel (for bytes).
    let (input_tx, input_rx) = channel_builder_large
        .with_labels(&["input"], true)
        .build_stream_bundle::<StreamIngress, 2>(1000);

    // Configure the actor builder with thread, load, and CPU telemetry.
    let actor_builder = graph.actor_builder()
        .with_thread_info()
        .with_mcpu_trigger(Trigger::AvgAbove(MCPU::m768()), AlertColor::Red)
        .with_mcpu_trigger(Trigger::AvgAbove(MCPU::m512()), AlertColor::Orange)
        .with_mcpu_trigger(Trigger::AvgAbove(MCPU::m256()), AlertColor::Yellow)
        .with_load_avg()
        .with_mcpu_avg();

    // === Aeron Input Configuration ===
    //
    // The Aeron channel is configured for IPC (inter-process communication) by default.
    // You can switch to UDP or multicast by uncommenting the relevant sections.
    // Aeron is a high-performance, low-latency messaging system ideal for distributed streaming.
    //
    let aeron_channel = AeronConfig::new()
        .with_media_type(MediaType::Ipc)
        .use_ipc()
        // .with_term_length((1024 * 1024 * 64) as usize)
        // .with_media_type(MediaType::Udp)
        // .use_point_to_point(Endpoint {
        //     ip: "127.0.0.1".parse().expect("Invalid IP address"),
        //     port: 40456,
        // })
        // .with_reliability(ReliableConfig::Reliable)
        .build();

    error!("subscribe to: {:?}", aeron_channel.cstring());

    // Build the aqueduct (Aeron input) actor, which receives data over Aeron.
    input_tx.build_aqueduct(
        AqueTech::Aeron(aeron_channel, 40),
        &mut actor_builder.with_name(NAME_AERON),
        SoloAct
    );

    // Build the deserialize actor, which splits and checks the incoming stream.
    let state = new_state();
    actor_builder.with_name(NAME_DESERIALIZE)
        .build(
            move |context| { actor::deserialize::run(context, input_rx.clone(), heartbeat_tx.clone(), generator_tx.clone(), state.clone()) },
            SoloAct
        );

    // Build the worker actor, which applies FizzBuzz logic.
    let state = new_state();
    actor_builder.with_name(NAME_WORKER)
        .build(
            move |context| { actor::worker::run(context, heartbeat_rx.clone(), generator_rx.clone(), worker_tx.clone(), state.clone()) },
            SoloAct
        );

    // Build the logger actor, which logs FizzBuzz results.
    let state = new_state();
    actor_builder.with_name(NAME_LOGGER)
        .build(
            move |context| { actor::logger::run(context, worker_rx.clone(), state.clone()) },
            SoloAct
        );
}

#[cfg(test)]
pub(crate) mod subscribe_main_tests {
    use std::time::Duration;
    use steady_state::*;
    use steady_state::graph_testing::{StageDirection, StageWaitFor};
    use crate::actor::worker::FizzBuzzMessage;
    use super::*;

    #[test]
    fn graph_test() -> Result<(), Box<dyn std::error::Error>> {
        // this is our special test graph without any barrier so we can shut down from the main thread.
        let mut graph = GraphBuilder::for_testing()
            .build(MainArg::default());

        build_graph(&mut graph);
        graph.start();

        let stage_manager = graph.stage_manager();

        let first_byte_arrival = Instant::now();
        let last_byte_finished = Instant::now();
        let sender_session_id = 8675309; // jenny

        //NOTE: we send 1 generated message and THEN the heartbeat to release it
        //      we also make sure the session_id matches and make up an arrival time of now
        stage_manager.actor_perform(NAME_AERON,
                                    StageDirection::EchoAt(1, StreamIngress::build(sender_session_id
                                                                                   ,first_byte_arrival,last_byte_finished
                                                                                   ,&[0, 0, 0, 0, 0, 0, 0, 15]))
        )?;
        //these messages are FAKE and did not come from aeron instead we INJECT them here like they arrived
        stage_manager.actor_perform(NAME_AERON,
                                    StageDirection::EchoAt(0, StreamIngress::build(sender_session_id
                                                                                   ,first_byte_arrival,last_byte_finished
                                                                                   ,&[0, 0, 0, 0, 0, 0, 0, 0]))
        )?;
        stage_manager.actor_perform(NAME_LOGGER,
                                    StageWaitFor::Message(FizzBuzzMessage::FizzBuzz, Duration::from_secs(60))
        )?;

        stage_manager.final_bow();
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(7))
    }
}
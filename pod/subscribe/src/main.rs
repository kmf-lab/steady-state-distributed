use std::env;
use steady_state::*;
use arg::MainArg;
mod arg;

pub(crate) mod actor {
    pub(crate) mod deserialize;
    pub(crate) mod worker;
    pub(crate) mod logger;
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        env::set_var("TELEMETRY_SERVER_PORT", "5552");
        env::set_var("TELEMETRY_SERVER_IP", "127.0.0.1");
    }

    let cli_args = MainArg::parse();
    let _ = init_logging(LogLevel::Info);
    //this is main() NOT in test so the barrier only effects release mode
    let mut graph = GraphBuilder::default()
        .with_shutdown_barrier(2)
        .build(cli_args);

    build_graph(&mut graph);

    graph.start();
    graph.block_until_stopped(std::time::Duration::from_secs(600))
}

/// Builds the graph for both normal operation and testing.
/// The aqueduct is included unconditionally, but in testing mode, its behavior is assumed to be mocked or isolated.
fn build_graph(graph: &mut Graph) {
    let channel_builder = graph.channel_builder()
        .with_filled_trigger(Trigger::AvgAbove(Filled::p90()), AlertColor::Red)
        .with_filled_trigger(Trigger::AvgAbove(Filled::p60()), AlertColor::Orange)
        .with_avg_filled()
        .with_avg_rate()
        .with_filled_percentile(Percentile::p80());

    let (input_tx, input_rx) = channel_builder
        .with_capacity(6400)
        .with_labels(&["input"], true)
        .build_stream_bundle::<StreamSessionMessage, 2>(1000);

    let (heartbeat_tx, heartbeat_rx) = channel_builder
        .with_labels(&["heartbeat"], true)
        .build_channel();
    let (generator_tx, generator_rx) = channel_builder
        .with_labels(&["generator"], true)
        .build_channel();
    let (worker_tx, worker_rx) = channel_builder.build_channel();

    let actor_builder = graph.actor_builder()
        .with_load_avg()
        .with_mcpu_avg();

    let aeron_channel = AeronConfig::new()
        .with_media_type(MediaType::Udp)
        .with_term_length((1024 * 1024 * 4) as usize)
        .use_point_to_point(Endpoint {
            ip: "127.0.0.1".parse().expect("Invalid IP address"),
            port: 40456,
        })
        .build();

    input_tx.build_aqueduct(
        AqueTech::Aeron(aeron_channel, 40),
        &mut actor_builder.with_name("aeron"),
        SoloAct
    );

    let steady = new_state();
    actor_builder.with_name("deserialize")
        .build(
            move |context| { actor::deserialize::run(context, input_rx.clone(), heartbeat_tx.clone(), generator_tx.clone(), steady.clone()) },
            SoloAct
        );

    actor_builder.with_name("worker")
        .build(
            move |context| { actor::worker::run(context, heartbeat_rx.clone(), generator_rx.clone(), worker_tx.clone()) },
            SoloAct
        );

    actor_builder.with_name("logger")
        .build(
            move |context| { actor::logger::run(context, worker_rx.clone()) },
            SoloAct
        );
}

#[cfg(test)]
pub(crate) mod main_tests {
   use std::time::Duration;
   use steady_state::*;
   use steady_state::graph_testing::{StageDirection, StageWaitFor};
   use crate::actor::worker::FizzBuzzMessage;
   use super::*;

    #[test]
    fn graph_test() -> Result<(), Box<dyn std::error::Error>> {
         // this is our special test graph without any barrier so we can shut down from the main thread.
        let mut graph = GraphBuilder::for_testing()
                                    .with_telemetry_metric_features(true)
                                    .build(MainArg::default());

        build_graph(&mut graph);
        graph.start();



        let stage_manager = graph.stage_manager();
        let now = Instant::now();
         //NOTE: we send 1 generated message and THEN the heartbeat to release it
        stage_manager.actor_perform("aeron",
              StageDirection::EchoAt(1, StreamSessionMessage::wrap(2,now,now,&[0, 0, 0, 0, 0, 0, 0, 42])) // Generator simulation
        )?;
        stage_manager.actor_perform("aeron",
              StageDirection::EchoAt(0, StreamSessionMessage::wrap(1,now,now,&[0, 0, 0, 0, 0, 0, 0, 0])) // Heartbeat simulation
        )?;

        error!("eeeeeeeeeeeeeee");
          stage_manager.actor_perform("logger",
               StageWaitFor::Message(FizzBuzzMessage::FizzBuzz, Duration::from_secs(30))
           )?;
 error!("xxxxxxxxxxxxxxxxx");         
         stage_manager.final_bow();
        error!("ssssssssssssss");
         graph.request_shutdown();
         graph.block_until_stopped(Duration::from_secs(1))
    }
}

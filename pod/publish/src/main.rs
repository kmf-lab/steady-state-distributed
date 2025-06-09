use std::env;
use steady_state::*;
//bring into lib
use arg::MainArg;
mod arg;

pub(crate) mod actor {
   pub(crate) mod heartbeat;
   pub(crate) mod generator;
   pub(crate) mod serialize;
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        env::set_var("TELEMETRY_SERVER_PORT", "5551");
        env::set_var("TELEMETRY_SERVER_IP", "127.0.0.1");
    }

    let cli_args = MainArg::parse();
    let _ = init_logging(LogLevel::Info);
    let mut graph = GraphBuilder::default()
           .with_shutdown_barrier(2) //TODO: explain this feature..
           .build(cli_args); //or pass () if no args

    build_graph(&mut graph);

    //startup entire graph
    graph.start();
    // your graph is running here until actor calls graph stop
    graph.block_until_stopped(std::time::Duration::from_secs(600))
}

fn build_graph(graph: &mut Graph) {
    let channel_builder = graph.channel_builder()
        .with_capacity(2_000_000)
        .with_filled_trigger(Trigger::AvgAbove(Filled::p90()), AlertColor::Red)
        .with_filled_trigger(Trigger::AvgAbove(Filled::p60()), AlertColor::Orange)
        .with_avg_filled()
        .with_avg_rate()
        .with_filled_percentile(Percentile::p80()); //is 128 need max of 100!!

    let (output_tx, output_rx) = channel_builder
        .build_stream_bundle::<StreamEgress, 2>(1000);

    let (heartbeat_tx, heartbeat_rx) = channel_builder.build_channel();
    let (generator_tx, generator_rx) = channel_builder.build_channel();

    let actor_builder = graph.actor_builder()
        .with_load_avg()
        .with_mcpu_avg();

    let state = new_state();
    actor_builder.with_name("heartbeat")
        .build(move |context| { actor::heartbeat::run(context, heartbeat_tx.clone(), state.clone()) }
               , SoloAct);

    let state = new_state();
    actor_builder.with_name("generator")
        .build(move |context| { actor::generator::run(context, generator_tx.clone(), state.clone()) }
               , SoloAct);

    actor_builder.with_name("serialize")
        .build(move |context| { actor::serialize::run(context, heartbeat_rx.clone(), generator_rx.clone(), output_tx.clone()) }
               , SoloAct);

    let aeron_channel = AeronConfig::new()
       // .with_media_type(MediaType::Ipc)
       // .use_ipc()

        .with_media_type(MediaType::Udp) //large term for greater volume
        .with_term_length((1024 * 1024 * 64) as usize)
        .use_point_to_point(Endpoint {
            ip: "127.0.0.1".parse().expect("Invalid IP address"),
            port: 40456,
        })

        .build();

    // let aeron_config = AeronConfig::new()
    //     .with_media_type(MediaType::Udp)
    //     .use_multicast(Endpoint {
    //         ip: "224.0.1.1".parse().expect("Invalid IP"),
    //         port: 40456,
    //     }, "eth0") // Specify network interface
    //     .build();



    error!("publish to: {:?}",aeron_channel.cstring());

    
    output_rx.build_aqueduct(AqueTech::Aeron(aeron_channel, 40)
                             , &mut actor_builder.with_name("publish")
                             , SoloAct
    );
}

#[cfg(test)]
pub(crate) mod publish_main_tests {
    use std::time::Duration;
    use steady_state::*;
    use steady_state::graph_testing::{StageDirection, StageWaitFor};
    use super::*;

    #[test]
    fn graph_test() -> Result<(), Box<dyn Error>> {
        // Initialize test graph with reasonable arguments
        let mut graph = GraphBuilder::for_testing()
                                     .build(MainArg::default());

        build_graph(&mut graph);
        graph.start();

        // Simulate actor behavior
        let stage_manager = graph.stage_manager();
        stage_manager.actor_perform("generator", StageDirection::EchoAt(0, 15u64))?;
        stage_manager.actor_perform("heartbeat", StageDirection::Echo(0u64))?;
        //in this test we do NOT send anything to aeron instead we confirm what we WOULD have sent matches our expectations.
        stage_manager.actor_perform( "publish", StageWaitFor::MessageAt(0, StreamEgress::build(&[0u8,0,0,0,0,0,0,0]), Duration::from_secs(4)))?;
        stage_manager.actor_perform( "publish", StageWaitFor::MessageAt(1, StreamEgress::build(&[0u8,0,0,0,0,0,0,15]), Duration::from_secs(4)))?;
        stage_manager.final_bow();
        
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(3))?;
        Ok(())
    }
// TODO: string constants!!
   
}



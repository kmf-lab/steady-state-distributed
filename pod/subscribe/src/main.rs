use std::env;
use steady_state::*;
use arg::MainArg;
mod arg;

pub(crate) mod actor {
    pub(crate) mod deserialize;
    pub(crate) mod worker;
    pub(crate) mod logger;
}

fn main() {

    unsafe {
        env::set_var("TELEMETRY_SERVER_PORT", "5552");
        env::set_var("TELEMETRY_SERVER_IP", "127.0.0.1");
    }

    let cli_args = MainArg::parse();
    let _ = init_logging(LogLevel::Info);
    let mut graph = GraphBuilder::default()
           .build(cli_args); //or pass () if no args


    build_graph(&mut graph);

    //startup entire graph
    graph.start();
    // your graph is running here until actor calls graph stop
    graph.block_until_stopped(std::time::Duration::from_secs(20));
}

fn build_graph(graph: &mut Graph) {
    let channel_builder = graph.channel_builder()
        .with_filled_trigger(Trigger::PercentileAbove(Percentile::p80(), Filled::p50()), AlertColor::Orange)
        .with_avg_filled()
        .with_filled_percentile(Percentile::p80());

    let (input_tx, input_rx) = channel_builder
        .with_avg_rate()
        .with_capacity(6400)
        .build_stream_bundle::<_, 2>(1000);

    let (heartbeat_tx, heartbeat_rx) = channel_builder
        .with_labels(&["heartbeat"], true)
        .build();
    let (generator_tx, generator_rx) = channel_builder
        .with_labels(&["generator"], true)
        .with_capacity(640).build();
    let (worker_tx, worker_rx) = channel_builder.build();

    let actor_builder = graph.actor_builder().with_mcpu_avg();

    let aeron_channel = AeronConfig::new()
        //.with_media_type(MediaType::Ipc)
        //.use_ipc()
        .with_media_type(MediaType::Udp)
        .with_term_length((1024 * 1024 * 4) as usize)
        .use_point_to_point(Endpoint {
            ip: "127.0.0.1".parse().expect("Invalid IP address"),
            port: 40456,
        })
        .build();

    input_tx.build_aqueduct(AqueTech::Aeron(aeron_channel, 40)
                            , &mut actor_builder.with_name("aeron")
                            , &mut Threading::Spawn
    );

    let mut team = ActorTeam::new(&graph);

    actor_builder.with_name("deserialize")
        .build(move |context| { actor::deserialize::run(context, input_rx.clone(), heartbeat_tx.clone(), generator_tx.clone()) }
               , &mut Threading::Spawn);

    // &mut Threading::Join(&mut team));

    actor_builder.with_name("worker")
        .build(move |context| { actor::worker::run(context, heartbeat_rx.clone(), generator_rx.clone(), worker_tx.clone()) }
               , &mut Threading::Spawn);

    //               , &mut Threading::Join(&mut team));
    //TODO: review this join code as teh possible issue.
    actor_builder.with_name("logger")
        .build(move |context| { actor::logger::run(context, worker_rx.clone()) }
               , &mut Threading::Spawn);

    //    , &mut Threading::Join(&mut team));

    team.spawn();
}
// TODO: need a unit test.


#[cfg(test)]
pub(crate) mod main_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;

    #[test]
    fn graph_test() {

        let gb = GraphBuilder::for_testing();

        let mut g = gb.build(MainArg { });

        build_graph(&mut g);

        g.start();
        
    }

}


//standard needs single message passing
//               graph test
//               actor test
//  demo something not send?
//  demo wait_for_all with multiple channels
//  demo state
//  demo clean shutdown
//  will be common base for the following 3
//  hb & gen ->try worker ->async logger/shutdown


// robust will have
//     panic, peek, dlq, externalAwait?

// performant will have
//     full batch usage, skip iterator ?
//     zero copy???visitor?

// distributed will have
//     stream demo between boxes
//



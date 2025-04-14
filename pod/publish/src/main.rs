use steady_state::*;
//bring into lib
use arg::MainArg;
mod arg;

pub(crate) mod actor {
   pub(crate) mod heartbeat;
   pub(crate) mod generator;
   pub(crate) mod serialize;
}

fn main() {
    let cli_args = MainArg::parse();
    let _ = init_logging(LogLevel::Info);
    let mut graph = GraphBuilder::default()
           .build(cli_args); //or pass () if no args

    let channel_builder = graph.channel_builder();

    let (output_tx,output_rx) = channel_builder.build_stream_bundle::<StreamSimpleMessage,2>(1000);

    let (heartbeat_tx,heartbeat_rx) = channel_builder.build_channel();
    let (generator_tx,generator_rx) = channel_builder.build_channel();

    let actor_builder = graph.actor_builder().with_mcpu_avg();

    let state = new_state();
    actor_builder.with_name("heartbeat")
         .build( move |context| { actor::heartbeat::run(context, heartbeat_tx.clone(), state.clone()) }
               , &mut Threading::Spawn);

    let state = new_state();
    actor_builder.with_name("generator")
        .build( move |context| { actor::generator::run(context, generator_tx.clone(), state.clone()) }
               , &mut Threading::Spawn);

    actor_builder.with_name("serialize")
        .build( move |context| { actor::serialize::run(context, heartbeat_rx.clone(), generator_rx.clone(), output_tx.clone()) }
               , &mut Threading::Spawn);

    let aeron_channel = AeronConfig::new()
        .with_media_type(MediaType::Ipc)
        .use_ipc()
        .build();

    output_rx.build_aqueduct(AqueTech::Aeron(aeron_channel,40)
                             ,&mut actor_builder.with_name("aeron")
                             ,&mut Threading::Spawn
                             );

    //startup entire graph
    graph.start();
    // your graph is running here until actor calls graph stop
    graph.block_until_stopped(std::time::Duration::from_secs(1));
}




//tests


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



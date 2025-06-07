use std::error::Error;
use steady_state::*;

pub(crate) async fn run(actor: SteadyActorShadow
                  , heartbeat: SteadyRx<u64>
                  , generator: SteadyRx<u64>
                  , output: SteadyStreamTxBundle<StreamEgress,2>) -> Result<(),Box<dyn Error>> {
    let cmd = actor.into_spotlight([&heartbeat, &generator],output.control_meta_data());
    internal_behavior(cmd, heartbeat, generator, output).await
}

async fn internal_behavior<A: SteadyActor>(mut actor: A
                                               , heartbeat: SteadyRx<u64>
                                               , generator: SteadyRx<u64>
                                               , output: SteadyStreamTxBundle<StreamEgress,2>) -> Result<(),Box<dyn Error>> {
    let mut rx_heartbeat = heartbeat.lock().await;
    let mut rx_generator = generator.lock().await;
    let mut output = output.lock().await; //private vec of guards of streams
    //pull out of vec of guards so we can give them great names
    let mut tx_generator = output.remove(1);//start at the end.
    let mut tx_heartbeat = output.remove(0);
    drop(output); //safety to ensure we do not use this again

    while actor.is_running(|| rx_heartbeat.is_closed_and_empty() && rx_generator.is_closed_and_empty() && tx_generator.mark_closed() && tx_heartbeat.mark_closed()) {

        await_for_any!(
            wait_for_all!(actor.wait_avail(&mut rx_heartbeat,1),actor.wait_vacant(&mut tx_heartbeat,(1,8))),
            wait_for_all!(actor.wait_avail(&mut rx_generator,1),actor.wait_vacant(&mut tx_generator,(1,8)))
        );

        if actor.vacant_units(&mut tx_heartbeat)>0 {
            if let Some(value) = actor.try_take(&mut rx_heartbeat) {
                let bytes = value.to_be_bytes();
                assert!(actor.try_send(&mut tx_heartbeat, &bytes).is_sent());
            };
        }

        while actor.vacant_units(&mut tx_generator)>0 {
            if let Some(value) = actor.try_take(&mut rx_generator) {
                let bytes = value.to_be_bytes();
                assert!(actor.try_send(&mut tx_generator, &bytes).is_sent());
            } else { 
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod serialize_tests {
     use super::*;

    #[cfg(test)]
    pub(crate) mod serialize_tests {
        use super::*;

        #[cfg(test)]
        pub(crate) mod serialize_tests {
            pub use std::thread::sleep;
            use steady_state::*;
            use crate::arg::MainArg;
            use super::*;

            #[test]
            fn test_serialize() -> Result<(), Box<dyn Error>> {
                let mut graph = GraphBuilder::for_testing().build(MainArg::default());

                let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
                let (generator_tx, generator_rx) = graph.channel_builder().build();
                let (stream_tx, stream_rx) = graph.channel_builder().build_stream_bundle::<_, 2>(8);

                graph.actor_builder()
                    .with_name("UnitTest")
                    .build(move |context|
                        internal_behavior(context, heartbeat_rx.clone(), generator_rx.clone(), stream_tx.clone())
                    , SoloAct);

                heartbeat_tx.testing_send_all(vec![0u64], true);
                generator_tx.testing_send_all(vec![42u64], true);

                graph.start();
                sleep(Duration::from_millis(100));
                graph.request_shutdown();
                graph.block_until_stopped(Duration::from_secs(1))?;

                let temp = vec!(StreamEgress::build(&[0, 0, 0, 0, 0, 0, 0, 0]));
                assert_steady_rx_eq_take!(&stream_rx[0], temp);
                assert_steady_rx_eq_take!(&stream_rx[1], vec!(StreamEgress::build(&[0, 0, 0, 0, 0, 0, 0, 42])));

                Ok(())
            }
        }


    }

}
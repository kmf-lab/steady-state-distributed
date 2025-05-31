use std::error::Error;
use steady_state::*;

pub(crate) async fn run(context: SteadyContext
                  , heartbeat: SteadyRx<u64>
                  , generator: SteadyRx<u64>
                  , output: SteadyStreamTxBundle<StreamSimpleMessage,2>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([&heartbeat, &generator],output.control_meta_data());
    internal_behavior(cmd, heartbeat, generator, output).await
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C
                                               , heartbeat: SteadyRx<u64>
                                               , generator: SteadyRx<u64>
                                               , output: SteadyStreamTxBundle<StreamSimpleMessage,2>) -> Result<(),Box<dyn Error>> {
    let mut rx_heartbeat = heartbeat.lock().await;
    let mut rx_generator = generator.lock().await;
    let mut output = output.lock().await; //private vec of guards of streams
    //pull out of vec of guards so we can give them great names
    let mut tx_generator = output.remove(1);//start at the end.
    let mut tx_heartbeat = output.remove(0);
    drop(output); //safety to ensure we do not use this again

    while cmd.is_running(|| rx_heartbeat.is_closed_and_empty() && rx_generator.is_closed_and_empty() && tx_generator.mark_closed() && tx_heartbeat.mark_closed()) {

        await_for_any!(
            wait_for_all!(cmd.wait_avail(&mut rx_heartbeat,1),cmd.wait_vacant(&mut tx_heartbeat,(1,8))),
            wait_for_all!(cmd.wait_avail(&mut rx_generator,1),cmd.wait_vacant(&mut tx_generator,(1,8)))
        );

        if cmd.vacant_units(&mut tx_heartbeat)>0 {
            if let Some(value) = cmd.try_take(&mut rx_heartbeat) {
                let bytes = value.to_be_bytes();
                assert!(cmd.try_send(&mut tx_heartbeat, &bytes).is_sent());
            };
        }

        while cmd.vacant_units(&mut tx_generator)>0 {
            if let Some(value) = cmd.try_take(&mut rx_generator) {
                let bytes = value.to_be_bytes();
                assert!(cmd.try_send(&mut tx_generator, &bytes).is_sent());
            } else { 
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod serialize_tests {
    pub use std::thread::sleep;
    use steady_state::*;
    use crate::arg::MainArg;
    use super::*;

    #[cfg(test)]
    pub(crate) mod serialize_tests {
        pub use std::thread::sleep;
        use steady_state::*;
        use steady_state::graph_testing::{StageDirection, StageWaitFor};
        use crate::arg::MainArg;
        use super::*;

        #[cfg(test)]
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

                // Send test data to inputs
                heartbeat_tx.testing_send_all(vec![0u64], true);
                generator_tx.testing_send_all(vec![42u64], true);

                graph.start();
                sleep(Duration::from_millis(100));
                graph.request_shutdown();
                graph.block_until_stopped(Duration::from_secs(1))?;

                // Verify serialized outputs - streams produce (StreamSimpleMessage, Box<[u8]>) tuples
                // 0u64.to_be_bytes() = [0, 0, 0, 0, 0, 0, 0, 0]
                // 42u64.to_be_bytes() = [0, 0, 0, 0, 0, 0, 0, 42]
                // let expected_heartbeat = (StreamSimpleMessage::new(8), vec![0, 0, 0, 0, 0, 0, 0, 0].into_boxed_slice());
                // let expected_generator = (StreamSimpleMessage::new(8), vec![0, 0, 0, 0, 0, 0, 0, 42].into_boxed_slice());
                // Verify serialized outputs - automatically detects streams!
                //assert_steady_rx_eq_take!(&stream_rx[0], vec![&[0, 0, 0, 0, 0, 0, 0, 0]]);
                //assert_steady_rx_eq_take!(&stream_rx[1], vec![&[0, 0, 0, 0, 0, 0, 0, 42]]);

                Ok(())
            }
        }


    }

}
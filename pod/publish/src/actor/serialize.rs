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
    error!("exited Ok");
    Ok(())
}

#[cfg(test)]
pub(crate) mod serialize_tests {
    pub use std::thread::sleep;
    use steady_state::*;
    use crate::arg::MainArg;
    use super::*;

    #[test]
    fn test_serialize() {
        let mut graph = GraphBuilder::for_testing().build(());
        //default capacity is 64 unless specified
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
        let (generator_tx, generator_rx) = graph.channel_builder().build();
        let (stream_tx, stream_rx) = graph.channel_builder().build_stream_bundle::< _, 2>(8);
     
        graph.actor_builder()
            .with_name("UnitTest")
            .build_spawn(move |context|
                internal_behavior(context, heartbeat_rx.clone(), generator_rx.clone(), stream_tx.clone())
            );           

        graph.start(); //startup the graph
        sleep(Duration::from_millis(1000 * 3)); //this is the default from args * 3
        graph.request_stop(); //our actor has no input so it immediately stops upon this request
        graph.block_until_stopped(Duration::from_secs(1));
     //   assert_steady_rx_eq_take!(&heartbeat_rx, vec!(0,1));
    }
}
use std::error::Error;
use std::thread::sleep;
use steady_state::*;
pub(crate) async fn run(context: SteadyContext
                  , input: SteadyStreamRxBundle<StreamSessionMessage,2>
                  , heartbeat: SteadyTx<u64>
                  , generator: SteadyTx<u64>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor(input.control_meta_data(),[&heartbeat, &generator]);
    internal_behavior(cmd, input, heartbeat, generator).await
}


async fn internal_behavior<T: SteadyCommander>(mut cmd: T
                           , input: SteadyStreamRxBundle<StreamSessionMessage,2>
                           , heartbeat: SteadyTx<u64>
                           , generator: SteadyTx<u64>) -> Result<(),Box<dyn Error>> {
    let mut input = input.lock().await;
    //pull out of vec of guards so we can give them great names
    let mut rx_generator = input.remove(1);//start at the end.
    let mut rx_heartbeat = input.remove(0);
    drop(input);
    let mut tx_heartbeat = heartbeat.lock().await;
    let mut tx_generator = generator.lock().await;
    let mut shutdown_count = 0; //TODO: move to steady?

    while cmd.is_running(|| rx_heartbeat.is_empty() && rx_generator.is_empty()
                        && tx_generator.mark_closed()
                        && tx_heartbeat.mark_closed()) {
         await_for_any!(
            //periodic!(Duration::from_secs(1)),
            wait_for_all!(cmd.wait_avail(&mut rx_heartbeat,1),cmd.wait_vacant(&mut tx_heartbeat,1)),
            wait_for_all!(cmd.wait_avail(&mut rx_generator,1),cmd.wait_vacant(&mut tx_generator,1))
         );

        if cmd.vacant_units(&mut tx_heartbeat)>0 {
            if let Some(bytes) = cmd.try_take(&mut rx_heartbeat) {
                // Ensure bytes.1 has exactly 8 bytes and convert to [u8; 8]
                let byte_array: [u8; 8] = bytes.1.as_ref().try_into().expect("Expected exactly 8 bytes");
                let beat = u64::from_be_bytes(byte_array);

                if u64::MAX == beat {
                    shutdown_count += 1;
                    error!("max beat found. shutdown count {}", shutdown_count);
                    if 2==shutdown_count {
                        cmd.request_graph_stop().await;
                    }         
                } else {
                    let _ = cmd.try_send(&mut tx_heartbeat, beat).is_sent();
                }
            }
        }

        if cmd.vacant_units(&mut tx_generator)>0  {
            if let Some(bytes) = cmd.try_take(&mut rx_generator) {
                         
                // Ensure bytes.1 has exactly 8 bytes and convert to [u8; 8]
                let byte_array: [u8; 8] = bytes.1.as_ref().try_into().expect("Expected exactly 8 bytes");
                let generated = u64::from_be_bytes(byte_array);
                if u64::MAX == generated {
                    shutdown_count += 1;                
                    error!("max generated found.  shutdown count {}", shutdown_count);
                    if 2==shutdown_count {
                        cmd.request_graph_stop().await;
                    }
                } else {
                    cmd.try_send(&mut tx_generator, generated).is_sent();
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod deserialize_tests {
    pub use std::thread::sleep;
    use steady_state::*;
    use crate::arg::MainArg;
    use super::*;

    #[test]
    fn test_deserialize() {
        let mut graph = GraphBuilder::for_testing().build(());
        //default capacity is 64 unless specified
        let (stream_tx, stream_rx) = graph.channel_builder().build_stream_bundle::<_, 2>(8);
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
        let (generator_tx, generator_rx) = graph.channel_builder().build();

        //TODO: write some serilized data

        graph.actor_builder()
            .with_name("UnitTest")
            .build_spawn(move |context|
                internal_behavior(context, stream_rx.clone(), heartbeat_tx.clone(), generator_tx.clone())
            );

        graph.start(); //startup the graph
        sleep(Duration::from_millis(1000 * 3)); //this is the default from args * 3
        graph.request_stop(); //our actor has no input so it immediately stops upon this request
        graph.block_until_stopped(Duration::from_secs(1));


        //TODO: assert we deserilized it
        // assert_steady_rx_eq_take!(&heartbeat_rx, vec!(0,1));
    }
}
use std::error::Error;
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
                    cmd.request_graph_stop();
                } else {
                    let _ = cmd.try_send(&mut tx_heartbeat, beat).is_sent();
                }
            }
        }

        if cmd.vacant_units(&mut tx_generator)>0  {
            if let Some(bytes) = cmd.try_take(&mut rx_generator) {
                // Ensure bytes.1 has exactly 8 bytes and convert to [u8; 8]
                let byte_array: [u8; 8] = bytes.1.as_ref().try_into().expect("Expected exactly 8 bytes");
                cmd.try_send(&mut tx_generator, u64::from_be_bytes(byte_array)).is_sent();
            }
        }


    }
    Ok(())
}
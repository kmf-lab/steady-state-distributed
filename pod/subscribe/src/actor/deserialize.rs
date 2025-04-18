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
    let mut heartbeat = heartbeat.lock().await;
    let mut generator = generator.lock().await;

    while cmd.is_running(|| input[0].is_empty() && generator.mark_closed() && heartbeat.mark_closed()) {
       //bug? await_for_any!(cmd.wait_avail_bundle(&mut input,1,1));//if 1 stream has 1 avail
       // await_for_any!(cmd.wait_avail(&mut input[0],1),
       //                 cmd.wait_avail(&mut input[1],1));
        await_for_any!(cmd.wait_periodic(Duration::from_millis(500)));

        if cmd.vacant_units(&mut heartbeat)>0 {
            if let Some(bytes) = cmd.try_take(&mut input[0]) {
                // Ensure bytes.1 has exactly 8 bytes and convert to [u8; 8]
                let byte_array: [u8; 8] = bytes.1.as_ref().try_into().expect("Expected exactly 8 bytes");
                let beat = u64::from_be_bytes(byte_array);
                if u64::MAX == beat {
                    cmd.request_graph_stop();
                } else {
                    let _ = cmd.try_send(&mut heartbeat, beat).is_sent();
                }
            }
        }

        while cmd.vacant_units(&mut generator)>0 && cmd.avail_units(&mut input[1])>0 {
            if let Some(bytes) = cmd.try_take(&mut input[1]) {
                // Ensure bytes.1 has exactly 8 bytes and convert to [u8; 8]
                let byte_array: [u8; 8] = bytes.1.as_ref().try_into().expect("Expected exactly 8 bytes");
                cmd.try_send(&mut generator, u64::from_be_bytes(byte_array)).is_sent();
            }
        }


    }
    Ok(())
}
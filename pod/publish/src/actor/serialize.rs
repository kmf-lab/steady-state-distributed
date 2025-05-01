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

    while cmd.is_running(|| rx_heartbeat.is_closed_and_empty() && rx_generator.is_closed() && output.mark_closed()) {

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

        if cmd.vacant_units(&mut tx_generator)>0 {
            if let Some(value) = cmd.try_take(&mut rx_generator) {
                let bytes = value.to_be_bytes();
                assert!(cmd.try_send(&mut tx_generator, &bytes).is_sent());
            };
        }

    }
    Ok(())
}
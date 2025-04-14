use std::error::Error;
use steady_state::*;

pub(crate) async fn run(context: SteadyContext
                  , heartbeat: SteadyRx<u64>
                  , generator: SteadyRx<u64>
                  , output: SteadyStreamTxBundle<StreamSimpleMessage,2>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([&heartbeat, &generator],output.control_meta_data());
    internal_behavior(cmd, heartbeat, generator, output).await
}

async fn internal_behavior<T: SteadyCommander>(mut cmd: T
                                               , heartbeat: SteadyRx<u64>
                                               , generator: SteadyRx<u64>
                                               , output: SteadyStreamTxBundle<StreamSimpleMessage,2>) -> Result<(),Box<dyn Error>> {
    let mut heartbeat = heartbeat.lock().await;
    let mut generator = generator.lock().await;
    let mut output = output.lock().await;

    while cmd.is_running(|| heartbeat.is_closed_and_empty() && generator.is_closed_and_empty() && output.mark_closed()) {
        // await until we have work to do
        await_for_any!(cmd.wait_avail(&mut heartbeat,1),
                       cmd.wait_avail(&mut generator,1));

        if cmd.vacant_units(&mut output[0])>0 {
            if let Some(value) = cmd.try_take(&mut heartbeat) {
                let bytes = value.to_be_bytes();
                assert!(cmd.try_send(&mut output[0], &bytes).is_sent());
            };
        }

        if cmd.vacant_units(&mut output[1])>0 {
            if let Some(value) = cmd.try_take(&mut generator) {
                let bytes = value.to_be_bytes();
                assert!(cmd.try_send(&mut output[1], &bytes).is_sent());
            };
        }

    }
    Ok(())
}
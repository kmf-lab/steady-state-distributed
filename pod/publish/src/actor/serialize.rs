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

        await_for_any!(cmd.wait_avail(&mut heartbeat,1),
                       cmd.wait_avail(&mut generator,1));

        if let Some(value) = cmd.try_peek(&mut heartbeat) {
            if cmd.wait_vacant(&mut output[0], (1, 8)).await {
               let bytes = value.to_be_bytes();
               assert!(cmd.try_send(&mut output[0], &bytes).is_sent());
               cmd.advance_read_index(&mut heartbeat, 1);
            }
        }
        if let Some(value) = cmd.try_peek(&mut generator) {
            if cmd.wait_vacant(&mut output[1], (1, 8)).await {
               let bytes = value.to_be_bytes();
               assert!(cmd.try_send(&mut output[1], &bytes).is_sent());
                cmd.advance_read_index(&mut generator, 1);
            }
        }
    }
    Ok(())
}
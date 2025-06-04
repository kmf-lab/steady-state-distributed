use steady_state::*;

/// by keeping the count in steady state this will not be lost or reset if this actor should panic
pub(crate) struct HeartbeatState {
    pub(crate) count: u64
}

/// this is the normal entry point for our actor in the graph using its normal implementation
pub async fn run(context: SteadyContext, heartbeat_tx: SteadyTx<u64>, state: SteadyState<HeartbeatState>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([], [&heartbeat_tx]);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, heartbeat_tx, state).await
    } else {
        cmd.simulated_behavior(vec!(&heartbeat_tx)).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C
                                               , heartbeat_tx: SteadyTx<u64>
                                               , state: SteadyState<HeartbeatState> ) -> Result<(),Box<dyn Error>> {
    let args = cmd.args::<crate::MainArg>().expect("unable to downcast");
    let rate = Duration::from_micros(args.rate_ms);
    let beats = args.beats;
 //   drop(args); //could be done this way
    let mut state = state.lock(|| HeartbeatState{ count: 0}).await;
    let mut heartbeat_tx = heartbeat_tx.lock().await;
    //loop is_running until shutdown signal then we call the closure which closes our outgoing Tx
    while cmd.is_running(|| heartbeat_tx.mark_closed()) {
        //await here until both of these are true
        await_for_all!(cmd.wait_periodic(rate),
                       cmd.wait_vacant(&mut heartbeat_tx, 1));

        let _ = cmd.try_send(&mut heartbeat_tx, state.count);
        state.count += 1;
        if beats == state.count {
            assert!(cmd.send_async(&mut heartbeat_tx, u64::MAX, SendSaturation::AwaitForRoom).await.is_sent());
            error!("Heartbet is done");
            //TODO: this is a hack to see if our shutdown is not waiting for the publsh to finish.
            cmd.wait(Duration::from_secs(10));
            cmd.request_shutdown().await;
        }
    }
    Ok(())
}

/// Here we test the internal behavior of this actor
#[cfg(test)]
pub(crate) mod heartbeat_tests {
    use steady_state::*;
    use crate::arg::MainArg;
    use super::*;

    #[test]
    fn test_heartbeat() -> Result<(), Box<dyn Error>> {
        let mut args = MainArg::default();
        args.beats = 3; // Set a small number for testing
        args.rate_ms = 10; // Fast rate for testing

        let mut graph = GraphBuilder::for_testing().build(args);
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();

        let state = new_state();
        graph.actor_builder()
            .with_name("UnitTest")
            .build(move |context| internal_behavior(context, heartbeat_tx.clone(), state.clone()), SoloAct);

        graph.start();
        std::thread::sleep(Duration::from_millis(100)); // Wait for a few beats
        graph.block_until_stopped(Duration::from_secs(1))?;

        let results = heartbeat_rx.testing_take_all();
        assert!(results.len() >= 2);
        assert_eq!(results[0], 0);
        assert_eq!(results[1], 1);
        Ok(())
    }

}

use steady_state::*;

/// by keeping the count in steady state this will not be lost or reset if this actor should panic
pub(crate) struct HeartbeatState {
    pub(crate) count: u64
}

/// this is the normal entry point for our actor in the graph using its normal implementation
pub async fn run(actor: SteadyActorShadow, heartbeat_tx: SteadyTx<u64>, state: SteadyState<HeartbeatState>) -> Result<(),Box<dyn Error>> {
    let actor = actor.into_spotlight([], [&heartbeat_tx]);
    if actor.use_internal_behavior {
        internal_behavior(actor, heartbeat_tx, state).await
    } else {
        actor.simulated_behavior(vec!(&heartbeat_tx)).await
    }
}

async fn internal_behavior<A: SteadyActor>(mut actor: A
                                               , heartbeat_tx: SteadyTx<u64>
                                               , state: SteadyState<HeartbeatState> ) -> Result<(),Box<dyn Error>> {
    let args = actor.args::<crate::MainArg>().expect("unable to downcast");
    let rate = Duration::from_micros(args.rate_ms);
    let beats = args.beats;
 //   drop(args); //could be done this way
    let mut state = state.lock(|| HeartbeatState{ count: 0}).await;
    let mut heartbeat_tx = heartbeat_tx.lock().await;
    //loop is_running until shutdown signal then we call the closure which closes our outgoing Tx
    while actor.is_running(|| heartbeat_tx.mark_closed()) {
        //await here until both of these are true
        await_for_all!(actor.wait_periodic(rate),
                       actor.wait_vacant(&mut heartbeat_tx, 1));

        if actor.try_send(&mut heartbeat_tx, state.count).is_sent() {
            state.count += 1;
            if beats == state.count {
                assert!(actor.send_async(&mut heartbeat_tx, u64::MAX, SendSaturation::AwaitForRoom).await.is_sent());
                error!("Heartbeat is done {}",state.count);
                actor.request_shutdown().await;
            }
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

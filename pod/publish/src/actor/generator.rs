use steady_state::*;

pub(crate) struct GeneratorState {
    pub(crate) value: u64
}

pub async fn run(context: SteadyContext, generated_tx: SteadyTx<u64>, state: SteadyState<GeneratorState>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([], [&generated_tx]);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, generated_tx, state).await
    } else {
        cmd.simulated_behavior(vec!(&TestEcho(generated_tx))).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C, generated: SteadyTx<u64>, state: SteadyState<GeneratorState> ) -> Result<(),Box<dyn Error>> {
    let args = cmd.args::<crate::MainArg>().expect("unable to downcast");
    let beats = args.beats;
    
    let mut state = state.lock(|| GeneratorState {value: 0}).await;
    let mut generated = generated.lock().await;

    const EXPECTED_UNITS_PER_BEAT:u64 = 3; //MUST MATCH THE CLIENT EXPECTATIONS
    while cmd.is_running(|| /*state.value >= beats*EXPECTED_UNITS_PER_BEAT &&*/ generated.mark_closed() ) {
         //this will await until we have room for this one.
         if cmd.send_async(&mut generated, state.value, SendSaturation::AwaitForRoom).await.is_sent() {
             state.value += 1;
             if beats*EXPECTED_UNITS_PER_BEAT == state.value {
                 assert!(cmd.send_async(&mut generated, u64::MAX, SendSaturation::AwaitForRoom).await.is_sent());
                 info!("request graph stop");
                 cmd.request_graph_stop().await;
             }
         }
    }
    Ok(())
}

/// Here we test the internal behavior of this actor
#[cfg(test)]
pub(crate) mod generator_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;

    #[test]
    fn test_generator() {
        let mut graph = GraphBuilder::for_testing().build(());
        let (generate_tx, generate_rx) = graph.channel_builder().build();

        let state = new_state();
        graph.actor_builder()
            .with_name("UnitTest")
            .build_spawn(move |context| internal_behavior(context, generate_tx.clone(), state.clone()) );

        graph.start();
        sleep(Duration::from_millis(100));
        graph.request_stop();

        graph.block_until_stopped(Duration::from_secs(1));

        assert_steady_rx_eq_take!(generate_rx,vec!(0,1));
    }
}
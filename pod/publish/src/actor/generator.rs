use steady_state::*;

// pub(crate) struct GeneratorState {
//     pub(crate) value: u64
// }
//
// const EXPECTED_UNITS_PER_BEAT:u64 = 800_000; //MUST MATCH THE CLIENT EXPECTATIONS
//
// pub async fn run(actor: SteadyActorShadow, generated_tx: SteadyTx<u64>, state: SteadyState<GeneratorState>) -> Result<(),Box<dyn Error>> {
//     let actor = actor.into_spotlight([], [&generated_tx]); //rename to into_spotlight
//     if actor.use_internal_behavior {
//         internal_behavior(actor, generated_tx, state).await
//     } else {
//         actor.simulated_behavior(vec!(&generated_tx)).await
//     }
// }
//
// async fn internal_behavior<A: SteadyActor>(mut actor: A, generated: SteadyTx<u64>, state: SteadyState<GeneratorState> ) -> Result<(),Box<dyn Error>> {
//     let args = actor.args::<crate::MainArg>().expect("unable to downcast");
//     let beats = args.beats;
//
//     let mut state = state.lock(|| GeneratorState {value: 0}).await;
//     let mut generated = generated.lock().await;
//
//     while actor.is_running(||  generated.mark_closed() ) {
//          //this will await until we have room for this one.
//          if actor.send_async(&mut generated, state.value, SendSaturation::AwaitForRoom).await.is_sent() {
//              state.value += 1;
//              if beats*EXPECTED_UNITS_PER_BEAT == state.value {
//                  assert!(actor.send_async(&mut generated, u64::MAX, SendSaturation::AwaitForRoom).await.is_sent());
//                  error!("Generator is done {}",state.value);
//                  actor.request_shutdown().await;
//              }
//          }
//     }
//     Ok(())
// }
//
// /// Here we test the internal behavior of this actor
// #[cfg(test)]
// pub(crate) mod generator_tests {
//     use steady_state::*;
//     use crate::arg::MainArg;
//     use super::*;
//     #[test]
//     fn test_generator() -> Result<(),Box<dyn Error>> {
//         let mut graph = GraphBuilder::for_testing().build(MainArg::default());
//         let (generate_tx, generate_rx) = graph.channel_builder().build();
//
//         let state = new_state();
//         graph.actor_builder()
//             .with_name("UnitTest")
//             .build(move |context| internal_behavior(context, generate_tx.clone(), state.clone()), SoloAct);
//
//         graph.start();
//
//         // Give it time to generate a few values, then stop
//         std::thread::sleep(Duration::from_millis(50));
//         graph.request_shutdown();
//         graph.block_until_stopped(Duration::from_secs(1))?;
//
//         // Should have generated at least a couple values
//         let results = generate_rx.testing_take_all();
//         assert!(results.len() >= 2);
//         assert_eq!(results[0], 0);
//         assert_eq!(results[1], 1);
//         Ok(())
//     }
//
// }


use steady_state::*;

pub(crate) struct GeneratorState {
    pub(crate) next_value: u64,
    pub(crate) batch_size: usize,
    pub(crate) total_generated: u64,
}

const EXPECTED_UNITS_PER_BEAT:u64 = 800_000; //MUST MATCH THE CLIENT EXPECTATIONS
//TODO: integratethe rest.
pub async fn run(actor: SteadyActorShadow, generated_tx: SteadyTx<u64>, state: SteadyState<GeneratorState>) -> Result<(),Box<dyn Error>> {
    let actor = actor.into_spotlight([], [&generated_tx]);
    if actor.use_internal_behavior {
        internal_behavior(actor, generated_tx, state).await
    } else {
        actor.simulated_behavior(vec!(&generated_tx)).await
    }
}

async fn internal_behavior<A: SteadyActor>(mut actor: A, generated: SteadyTx<u64>, state: SteadyState<GeneratorState> ) -> Result<(),Box<dyn Error>> {

    let mut generated = generated.lock().await;

    let mut state = state.lock(|| GeneratorState {
        next_value: 0,
        batch_size: generated.capacity()/2, // Large batch size for high throughput
        total_generated: 0,
    }).await;


    // Pre-allocate batch buffer to avoid repeated allocations
    let mut batch = Vec::with_capacity(state.batch_size);

    while actor.is_running(|| i!(generated.mark_closed())) {
        // Wait for sufficient room in channel for our batch
        await_for_all!(actor.wait_vacant(&mut generated, state.batch_size));

        // Prepare a full batch of values
        batch.clear();
        for _ in 0..state.batch_size {
            batch.push(state.next_value);
            state.next_value += 1;
        }

        // Send the entire batch at once for maximum throughput
        let sent_count = actor.send_slice_until_full(&mut generated, &batch);
        state.total_generated += sent_count as u64;

        if sent_count < batch.len() {
            // Channel became full, adjust next_value to account for unsent messages
            state.next_value -= (batch.len() - sent_count) as u64;
        }

        // Log throughput periodically
        if state.total_generated % 10000 == 0 {
            trace!("Generator: {} total messages sent", state.total_generated);
        }
    }

    info!("Generator shutting down. Total generated: {}", state.total_generated);
    Ok(())
}

/// Here we test the internal behavior of this actor
#[cfg(test)]
pub(crate) mod generator_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;

    #[test]
    fn test_generator() -> Result<(), Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(());
        let (generate_tx, generate_rx) = graph.channel_builder()
            .with_capacity(1024) // Larger capacity for testing
            .build();

        let state = new_state();
        graph.actor_builder()
            .with_name("UnitTest")
            .build(move |context| internal_behavior(context, generate_tx.clone(), state.clone()), SoloAct );

        graph.start();
        sleep(Duration::from_millis(100));
        graph.request_shutdown();

        graph.block_until_stopped(Duration::from_secs(1))?;

        // Should have many more messages due to batch processing
        let messages: Vec<u64> = generate_rx.testing_take_all();
        assert!(messages.len() >= 256); // At least one full batch
        assert_eq!(messages[0], 0);
        assert_eq!(messages[1], 1);
        Ok(())
    }
}

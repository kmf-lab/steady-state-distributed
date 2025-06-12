use std::error::Error;
use steady_state::*;
use crate::actor::worker;

pub(crate) struct DeserializeState {
    shutdown_count: i32,
    batch_size: usize,
}

pub(crate) async fn run(
    actor: SteadyActorShadow,
    input: SteadyStreamRxBundle<StreamIngress, 2>,
    heartbeat: SteadyTx<u64>,
    generator: SteadyTx<u64>,
    state: SteadyState<DeserializeState>,
) -> Result<(), Box<dyn Error>> {
    let actor = actor.into_spotlight(input.payload_meta_data(), [&heartbeat, &generator]); // payload for aqueduct??
    internal_behavior(actor, input, heartbeat, generator, state).await
}

async fn internal_behavior<A: SteadyActor>(
    mut actor: A,
    input: SteadyStreamRxBundle<StreamIngress, 2>,
    heartbeat: SteadyTx<u64>,
    generator: SteadyTx<u64>,
    state: SteadyState<DeserializeState>,
) -> Result<(), Box<dyn Error>> {

    let mut input = input.lock().await;
    let mut rx_generator = input.remove(1); // Stream 1 for generator
    let mut rx_heartbeat = input.remove(0); // Stream 0 for heartbeat
    drop(input); // Safety to prevent accidental reuse
    let mut tx_heartbeat = heartbeat.lock().await;
    let mut tx_generator = generator.lock().await;


    let mut state = state.lock(|| DeserializeState {
        shutdown_count: 0,
        batch_size: worker::BATCH_SIZE.min(rx_generator.capacity() / worker::SLICES),
    }).await;

    let mut tx_batch = vec![0u64; state.batch_size];

    while actor.is_running(|| {
               rx_heartbeat.is_empty()
            && rx_generator.is_empty()
            && tx_generator.mark_closed()
            && tx_heartbeat.mark_closed()
    }) {
        await_for_any!(
            wait_for_all!(
                actor.wait_avail(&mut rx_heartbeat, 1),
                actor.wait_vacant(&mut tx_heartbeat, 1)
            ),
            wait_for_all!(
                actor.wait_avail(&mut rx_generator, 1),
                actor.wait_vacant(&mut tx_generator, 1)
            )
        );

        let mut units_count = actor.vacant_units(&mut tx_heartbeat)
                                 .min(actor.avail_units(&mut rx_heartbeat));

        while units_count > 0 {
            if let Some(bytes) = actor.try_take(&mut rx_heartbeat) {
                let byte_array: [u8; 8] = bytes
                    .1
                    .as_ref()
                    .try_into()
                    .expect("Expected exactly 8 bytes");
                let beat = u64::from_be_bytes(byte_array);

                if beat == u64::MAX {
                    state.shutdown_count += 1;
                    if state.shutdown_count == 2 {
                        actor.request_shutdown().await;
                    }
                } else {
                    assert!(actor.try_send(&mut tx_heartbeat, beat).is_sent());
                }
            }
            units_count -= 1;
        }

        //TODO: missing needed method?
        //let _ = actor.take_slice(&mut rx_generator, &mut generator_batch[0..gen_count]);

        loop {
            let units_count =
                           tx_batch.len()
                          .min(actor.vacant_units(&mut tx_generator))
                          .min(actor.avail_units(&mut rx_generator));
            
            if 0==units_count {
                break;
            }
            // trace!("deserialize count {}",units_count);
            
            let mut idx = 0;
            while idx<units_count {
                //TODO: this gave us the %work but is not making uCPU, notsure why..
                actor.relay_stats_smartly(); // TODO: we are not getting cpu and we did nto get error on missing await

                //TODO: missing slice take of bytes?
                if let Some(bytes) = actor.try_take(&mut rx_generator) {
                    let byte_array: [u8; 8] = bytes
                        .1
                        .as_ref()
                        .try_into()
                        .expect("Expected exactly 8 bytes");
                    let generated = u64::from_be_bytes(byte_array);
                    if generated == u64::MAX {
                        state.shutdown_count += 1;
                        if state.shutdown_count == 2 {
                            actor.request_shutdown().await;
                        }
                    } else {
                        tx_batch[idx] = generated;
                    }
                } else {
                    break;
                }
                idx += 1;
            }
            assert_eq!(idx, actor.send_slice(&mut tx_generator, &mut tx_batch[0..idx]).item_count());
            
            // trace!("--------------------- done");
        }

    }

    Ok(())
}

#[cfg(test)]
pub(crate) mod deserialize_tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;
    use crate::arg::MainArg;

    #[test]
    fn test_deserialize() -> Result<(), Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(MainArg::default());

        let (stream_tx, stream_rx) = graph.channel_builder().build_stream_bundle::<_, 2>(8);
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
        let (generator_tx, generator_rx) = graph.channel_builder().build();

        let state = new_state();
        graph
            .actor_builder()
            .with_name("UnitTest")
            .build(
                move |context| {
                    internal_behavior(
                        context,
                        stream_rx.clone(),
                        heartbeat_tx.clone(),
                        generator_tx.clone(),
                        state.clone(),
                    )
                },
                SoloAct,
            );

        // Send serialized data matching serialize.rs test output
        let now = Instant::now();
        stream_tx[0].testing_send_all(vec![StreamIngress::by_ref(1,now,now,&[0, 0, 0, 0, 0, 0, 0, 0])], true);
        stream_tx[1].testing_send_all(vec![StreamIngress::by_ref(2,now,now,&[0, 0, 0, 0, 0, 0, 0, 42])], true);

        graph.start();
        sleep(Duration::from_millis(100)); // Match serialize.rs timing
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;

        assert_steady_rx_eq_take!(&heartbeat_rx, vec![0u64]);
        assert_steady_rx_eq_take!(&generator_rx, vec![42u64]);

        Ok(())
    }
}
use std::error::Error;
use steady_state::*;
use steady_state::distributed::distributed_stream::StreamControlItem; //TODO: make pub
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

    let mut control_batch = vec![StreamIngress::default(); state.batch_size];
    let mut payload_batch          = vec![0u8; state.batch_size * 8];
    let mut output_batch          = vec![0u64; state.batch_size];

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

        let mut units_count = state.batch_size
                                   .min(actor.vacant_units(&mut tx_heartbeat))
                                   .min(actor.avail_units(&mut rx_heartbeat));

        let done = actor.take_slice(&mut rx_heartbeat, ( &mut control_batch[0..units_count], &mut payload_batch[0.. ]  ));

        let mut byte_pos = 0;
        let mut output_idx = 0;
        for i in (0..done.item_count()) {
            //one control message could hold many values to deserialize
            let mut len = control_batch[i].length(); //TODO: strange import needed?    TODO: units on the chart would also be nice, and thicker lines.
            while len>0 {
                let slice = &payload_batch[byte_pos..(byte_pos+8)];
                let beat = u64::from_be_bytes(slice.try_into().expect("Internal Error"));

                if beat == u64::MAX {
                    state.shutdown_count += 1;
                    if state.shutdown_count == 2 {
                        actor.request_shutdown().await;
                    }
                } else {
                    output_batch[output_idx] = beat;
                    output_idx += 1;
                }
                byte_pos += 8;
                len -= 8;
            }
        }
        if output_idx>0 {
             actor.send_slice(&mut tx_heartbeat, &output_batch[0..output_idx]);

            // for i in 0..output_idx {
            //     actor.try_send(&mut tx_heartbeat, output_batch[i]);
            //
            // }


        }

        //////////////////////////////////////////////////

        let mut units_count = state.batch_size
            .min(actor.vacant_units(&mut tx_generator))
            .min(actor.avail_units(&mut rx_generator));

        let done = actor.take_slice(&mut rx_generator, ( &mut control_batch[0..units_count], &mut payload_batch[0.. ]  ));

        let mut byte_pos = 0;
        let mut output_idx = 0;
        for i in (0..done.item_count()) {
            //one control message could hold many values to deserialize
            let mut len = control_batch[i].length(); //TODO: strange import needed?
            while len>0 {
                let slice = &payload_batch[byte_pos..(byte_pos+8)];
                let beat = u64::from_be_bytes(slice.try_into().expect("Internal Error"));

                if beat == u64::MAX {
                    state.shutdown_count += 1;
                    if state.shutdown_count == 2 {
                        actor.request_shutdown().await;
                    }
                } else {
                    output_batch[output_idx] = beat;
                    output_idx += 1;
                }
                byte_pos += 8;
                len -= 8;
            }
        }
        if output_idx>0 {
           actor.send_slice(&mut tx_generator, &output_batch[0..output_idx]);

            // for i in 0..output_idx {
            //     actor.try_send(&mut tx_heartbeat, output_batch[i]);
            //
            // }
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
use std::error::Error;
use steady_state::*;
use steady_state::distributed::distributed_stream::StreamControlItem;

pub(crate) async fn run(actor: SteadyActorShadow
                        , heartbeat: SteadyRx<u64>
                        , generator: SteadyRx<u64>
                        , output: SteadyStreamTxBundle<StreamEgress,2>) -> Result<(),Box<dyn Error>> {
    let cmd = actor.into_spotlight([&heartbeat, &generator],output.payload_meta_data()); //must match the aqueduct??
    internal_behavior(cmd, heartbeat, generator, output).await
}

const MAX_GROUP: i32 = 8188;// for 64K blocks on look back,  1122 for 9K jumbo packets;

async fn internal_behavior<A: SteadyActor>(mut actor: A
                                               , heartbeat: SteadyRx<u64>
                                               , generator: SteadyRx<u64>
                                               , output: SteadyStreamTxBundle<StreamEgress,2>) -> Result<(),Box<dyn Error>> {
    let mut rx_heartbeat = heartbeat.lock().await;
    let mut rx_generator = generator.lock().await;
    let mut output = output.lock().await; //private vec of guards of streams
    //pull out of vec of guards so we can give them great names
    let mut tx_generator = output.remove(1);//start at the end.
    let mut tx_heartbeat = output.remove(0);
    drop(output); //safety to ensure we do not use this again

    let in_batch = rx_generator.capacity()/2;

    let mut rx_batch = vec![0u64; in_batch];

    let mut input_batch          = vec![0u64; in_batch];
    let mut control_batch = vec![StreamEgress::default(); in_batch];
    let mut payload_batch          = vec![0u8; in_batch * 8];


    while actor.is_running(|| rx_heartbeat.is_closed_and_empty() && rx_generator.is_closed_and_empty() && tx_generator.mark_closed() && tx_heartbeat.mark_closed()) {
        await_for_any!(
            wait_for_all!(actor.wait_avail(&mut rx_heartbeat,1),actor.wait_vacant(&mut tx_heartbeat,(1,8))),
            wait_for_all!(actor.wait_avail(&mut rx_generator,1),actor.wait_vacant(&mut tx_generator,(1,8)))
        );


        let mut units_count = in_batch.min(actor.vacant_units(&mut tx_heartbeat))
            .min(actor.avail_units(&mut rx_heartbeat));

        // error!("send units {}", units_count);
        let done = actor.take_slice(&mut rx_heartbeat, (&mut input_batch[0..units_count]));
        let mut payload_idx = 0;
        let mut control_idx = 0;
        let mut group_count = 0;
        for i in (0..done.item_count()) {
            let bytes = input_batch[i].to_be_bytes();
            payload_batch[payload_idx..(payload_idx + 8)].copy_from_slice(&bytes);
            payload_idx += 8;
            group_count += 1;
            if 5 == group_count {
                control_batch[control_idx] = StreamEgress::new(8 * group_count);
                control_idx += 1;
                group_count = 0;
            }
        }
        if group_count > 0 {
            control_batch[control_idx] = StreamEgress::new(8 * group_count);
            control_idx += 1;
        }
        let result = actor.send_slice(&mut tx_heartbeat, (&control_batch[0..control_idx], &payload_batch[0..payload_idx]));



        //////////////////////////////////////////////////////

        let mut units_count = in_batch.min(actor.vacant_units(&mut tx_generator))
            .min(actor.avail_units(&mut rx_generator));

        // error!("send units {}", units_count);

        let done = actor.take_slice(&mut rx_generator, (&mut input_batch[0..units_count]));
        let mut payload_idx = 0;
        let mut control_idx = 0;
        let mut group_count = 0;
        for i in (0..done.item_count()) {
            let bytes = input_batch[i].to_be_bytes();
            payload_batch[payload_idx..(payload_idx + 8)].copy_from_slice(&bytes);
            payload_idx += 8;
            group_count += 1;
            if MAX_GROUP == group_count {
                control_batch[control_idx] = StreamEgress::new(8 * group_count);
                control_idx += 1;
                group_count = 0;
            }
        }
        if group_count > 0 {
            control_batch[control_idx] = StreamEgress::new(8 * group_count);
            control_idx += 1;
        }

        let done = actor.send_slice(&mut tx_generator, (&control_batch[0..control_idx], &payload_batch[0..payload_idx]));


    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod serialize_tests {
     use super::*;

    #[cfg(test)]
    pub(crate) mod serialize_tests {
        use super::*;

        #[cfg(test)]
        pub(crate) mod serialize_tests {
            pub use std::thread::sleep;
            use steady_state::*;
            use crate::arg::MainArg;
            use super::*;

            #[test]
            fn test_serialize() -> Result<(), Box<dyn Error>> {
                let mut graph = GraphBuilder::for_testing().build(MainArg::default());

                let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
                let (generator_tx, generator_rx) = graph.channel_builder().build();
                let (stream_tx, stream_rx) = graph.channel_builder().build_stream_bundle::<_, 2>(8);

                graph.actor_builder()
                    .with_name("UnitTest")
                    .build(move |context|
                        internal_behavior(context, heartbeat_rx.clone(), generator_rx.clone(), stream_tx.clone())
                    , SoloAct);

                heartbeat_tx.testing_send_all(vec![0u64], true);
                generator_tx.testing_send_all(vec![42u64], true);

                graph.start();
                sleep(Duration::from_millis(100));
                graph.request_shutdown();
                graph.block_until_stopped(Duration::from_secs(1))?;

                let temp = vec!(StreamEgress::build(&[0, 0, 0, 0, 0, 0, 0, 0]));
                assert_steady_rx_eq_take!(&stream_rx[0], temp);
                assert_steady_rx_eq_take!(&stream_rx[1], vec!(StreamEgress::build(&[0, 0, 0, 0, 0, 0, 0, 42])));

                Ok(())
            }
        }


    }

}
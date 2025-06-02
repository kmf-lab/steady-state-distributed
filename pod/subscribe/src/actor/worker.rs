
use steady_state::*;

// over designed this enum is. much to learn here we have.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
#[repr(u64)] // Pack everything into 8 bytes
pub(crate) enum FizzBuzzMessage {
    #[default]
    FizzBuzz = 15,         // Discriminant is 15 - could have been any valid FizzBuzz
    Fizz = 3,              // Discriminant is 3 - could have been any valid Fizz
    Buzz = 5,              // Discriminant is 5 - could have been any valid Buzz
    Value(u64),            // Store u64 directly, use the fact that FizzBuzz/Fizz/Buzz only occupy small values
}
impl FizzBuzzMessage {
    pub fn new(value: u64) -> Self {
        match (value % 3, value % 5) {
            (0, 0) => FizzBuzzMessage::FizzBuzz,    // Multiple of 15
            (0, _) => FizzBuzzMessage::Fizz,        // Multiple of 3, not 5
            (_, 0) => FizzBuzzMessage::Buzz,        // Multiple of 5, not 3
            _      => FizzBuzzMessage::Value(value), // Neither
        }
    }
}

pub async fn run(context: SteadyContext
                 , heartbeat: SteadyRx<u64> //the type can be any struct or primitive or enum...
                 , generator: SteadyRx<u64>
                 , logger: SteadyTx<FizzBuzzMessage>) -> Result<(),Box<dyn Error>> {
    internal_behavior(context.into_monitor([&heartbeat, &generator], [&logger]), heartbeat, generator, logger).await
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C
                                               , heartbeat: SteadyRx<u64> //the type can be any struct or primitive or enum...
                                               , generator: SteadyRx<u64>
                                               , logger: SteadyTx<FizzBuzzMessage>) -> Result<(),Box<dyn Error>> {

    let mut heartbeat_rx = heartbeat.lock().await;
    let mut generator_rx = generator.lock().await;
    let mut logger_tx = logger.lock().await;

    while cmd.is_running(|| { 
                         //trace!("shutdown requested {} {}",heartbeat_rx.is_closed(), generator_rx.is_closed());
                         heartbeat_rx.is_closed() && generator_rx.is_closed() && logger_tx.mark_closed()}) {
        let mut count_down_items_per_tick = 3; //if too big we might hang
        let _clean =  await_for_all!(cmd.wait_vacant(&mut logger_tx, 1),
                                     cmd.wait_avail(&mut heartbeat_rx, 1),
                                     cmd.wait_avail(&mut generator_rx, count_down_items_per_tick)
                                  );

        if let Some(_h) = cmd.try_take(&mut heartbeat_rx) {
            //for each beat we empty the generated data
            //try to take count if possible, but no more, do NOT use take_into_iterator as it consumes all it sees.
            while let Some(item) = cmd.try_take(&mut generator_rx)  {
                //note: SendSaturation tells the async call to just wait if the outgoing channel
                //      is full. Another popular choice is Warn so it logs if it gets filled.
                let result = cmd.send_async(&mut logger_tx, FizzBuzzMessage::new(item)
                               , SendSaturation::AwaitForRoom).await;
                if let SendOutcome::Blocked(_d) = result {
                    //note: we already consumed d so it is lost but we know we are shutting down now.
                    break;
                }
                count_down_items_per_tick -=1;
                if 0 == count_down_items_per_tick {
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod worker_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;

    #[test]
    fn test_worker() -> Result<(),Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(());
        let (generate_tx, generate_rx) = graph.channel_builder().build();
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
        let (logger_tx, logger_rx) = graph.channel_builder().build::<FizzBuzzMessage>();

        graph.actor_builder().with_name("UnitTest")
             .build(move |context| internal_behavior(context
                                                             , heartbeat_rx.clone()
                                                             , generate_rx.clone()
                                                             , logger_tx.clone())
                 , SoloAct
             );

        generate_tx.testing_send_all(vec![0,1,2,3,4,5], true);
        heartbeat_tx.testing_send_all(vec![0,1], true);
        graph.start();

        sleep(Duration::from_millis(500));

        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;
        assert_steady_rx_eq_take!(&logger_rx, [FizzBuzzMessage::FizzBuzz
                                              ,FizzBuzzMessage::Value(1)
                                              ,FizzBuzzMessage::Value(2)
                                              ,FizzBuzzMessage::Fizz
                                              ,FizzBuzzMessage::Value(4)
                                              ,FizzBuzzMessage::Buzz]);
        Ok(())
    }
}
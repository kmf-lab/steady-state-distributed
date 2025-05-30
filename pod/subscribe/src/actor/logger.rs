use std::sync::{Mutex};
use steady_state::*;
use crate::actor::worker::FizzBuzzMessage;

pub async fn run(context: SteadyContext, fizz_buzz_rx: SteadyRx<FizzBuzzMessage>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([&fizz_buzz_rx], []);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, fizz_buzz_rx).await
    } else {
        cmd.simulated_behavior(vec!(&fizz_buzz_rx)).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C, rx: SteadyRx<FizzBuzzMessage>) -> Result<(),Box<dyn Error>> {
    let mut rx = rx.lock().await;
    let mut count = 0;
    while cmd.is_running(|| rx.is_closed_and_empty()) {
        await_for_all!(cmd.wait_avail(&mut rx, 1));
        count += 1;
        if let Some(msg) = cmd.try_take(&mut rx) {
            if (count < 100) {
                info!("Msg {:?}", msg );
            }
    }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod logger_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;
    #[test]
    fn test_logger() -> Result<(), Box<dyn std::error::Error>> {
        use steady_logger::*;

        initialize_with_level(LogLevel::Trace).expect("Failed to initialize test logger");
        let _guard = start_log_capture();

        let mut graph = GraphBuilder::for_testing().build(());
        let (fizz_buzz_tx, fizz_buzz_rx) = graph.channel_builder().build();

        graph.actor_builder().with_name("UnitTest")
            .build(move |context| internal_behavior(context, fizz_buzz_rx.clone())
                   , SoloAct);

        graph.start();
        fizz_buzz_tx.testing_send_all(vec![FizzBuzzMessage::Fizz], true);
        sleep(Duration::from_millis(300));
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;

        assert_in_logs!(vec!["Msg Fizz"]);
        Ok(())
    }
}
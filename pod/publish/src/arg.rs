use clap::Parser;

#[derive(Parser, Debug, PartialEq, Clone)]
pub(crate) struct MainArg {
    #[arg(short = 'r', long = "rate", default_value = "100")] //micro seconds 1000 per ms
    pub(crate) rate_ms: u64,
    #[arg(short = 'b', long = "beats", default_value = "60000")]
    pub(crate) beats: u64,
}
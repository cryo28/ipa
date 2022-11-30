use clap::Parser;
use hyper::http::uri::Scheme;
use rand::thread_rng;
use raw_ipa::{
    cli::Verbosity,
    ff::Fp31,
    helpers::{
        http::{HttpHelper, PrssExchangeStep},
        GatewayConfig, Role, SendBufferConfig,
    },
    net::discovery,
    protocol::{boolean::random_bits_generator::RandomBitsGenerator, QueryId, Step},
};
use std::error::Error;
use std::str::FromStr;
use tracing::info;

#[derive(Debug, Parser)]
#[clap(name = "mpc-helper", about = "CLI to start an MPC helper endpoint")]
struct Args {
    /// Configure logging.
    #[clap(flatten)]
    logging: Verbosity,

    /// Indicates which role this helper is
    #[arg(short, long, value_enum)]
    role: Role,

    /// Port to listen. If not specified, will ask Kernel to assign the port
    #[arg(short, long)]
    port: Option<u16>,

    /// Indicates whether to start HTTP or HTTPS endpoint
    #[arg(short, long, default_value = "http")]
    scheme: Scheme,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let _handle = args.logging.setup_logging();

    let peer_discovery_str =
        std::fs::read_to_string("./peer_conf.toml").expect("unable to read file");
    let peer_discovery =
        discovery::conf::Conf::from_str(&peer_discovery_str).expect("unable to parse config file");
    let gateway_config = GatewayConfig {
        send_buffer_config: SendBufferConfig {
            items_in_batch: 1,
            batch_count: 40,
        },
    };
    let helper = HttpHelper::new(args.role, &peer_discovery, gateway_config);
    let (addr, server_handle) = helper.bind().await;
    let gateway = helper.query(QueryId).expect("unable to create gateway");
    let step = Step::default().narrow(&PrssExchangeStep);
    let prss_endpoint = helper
        .prss_endpoint(&gateway, &step, &mut thread_rng())
        .await
        .expect("unable to setup prss");
    let rbg = RandomBitsGenerator::<Fp31>::new();
    let _ctx = helper.context(&gateway, &prss_endpoint, &rbg);

    info!(
        "listening to {}://{}, press Enter to quit",
        args.scheme, addr
    );
    let _ = std::io::stdin().read_line(&mut String::new())?;
    server_handle.abort();

    Ok(())
}

use clap::{Parser, Subcommand};
use comfy_table::{Cell, Color, Table};
use generic_array::ArrayLength;
use hyper::http::uri::Scheme;
use ipa::{
    cli::{
        playbook::{secure_mul, semi_honest, InputSource},
        CsvSerializer, Verbosity,
    },
    config::{ClientConfig, NetworkConfig, PeerConfig},
    ff::{Field, FieldType, Fp31, Fp32BitPrime, Serializable},
    helpers::query::{IpaQueryConfig, QueryConfig, QueryType},
    net::{ClientIdentity, MpcHelperClient},
    protocol::{BreakdownKey, MatchKey, QueryId},
    secret_sharing::{replicated::semi_honest::AdditiveShare, IntoShares},
    test_fixture::{
        ipa::{ipa_in_the_clear, TestRawDataRecord},
        EventGenerator, EventGeneratorConfig,
    },
};
use rand::thread_rng;
use std::{
    error::Error,
    fmt::Debug,
    fs,
    fs::OpenOptions,
    io,
    io::{stdout, Write},
    ops::Add,
    path::PathBuf,
    time::Duration,
};
use tokio::time::sleep;

#[derive(Debug, Parser)]
#[clap(
    name = "mpc-client",
    about = "CLI to execute test scenarios on IPA MPC helpers"
)]
#[command(about)]
struct Args {
    #[clap(flatten)]
    logging: Verbosity,

    /// Path to helper network configuration file
    #[arg(long)]
    network: Option<PathBuf>,

    /// Use insecure HTTP
    #[arg(short = 'k', long)]
    disable_https: bool,

    /// Seconds to wait for server to be running
    #[arg(short, long, default_value_t = 0)]
    wait: usize,

    #[clap(flatten)]
    input: CommandInput,

    #[command(subcommand)]
    action: TestAction,
}

#[derive(Debug, Parser)]
pub struct CommandInput {
    #[arg(
        long,
        help = "Read the input from the provided file, instead of standard input"
    )]
    input_file: Option<PathBuf>,

    #[arg(value_enum, long, default_value_t = FieldType::Fp32BitPrime, help = "Convert the input into the given field before sending to helpers")]
    field: FieldType,
}

impl From<&CommandInput> for InputSource {
    fn from(source: &CommandInput) -> Self {
        if let Some(ref file_name) = source.input_file {
            InputSource::from_file(file_name)
        } else {
            InputSource::from_stdin()
        }
    }
}

#[derive(Debug, Subcommand)]
enum TestAction {
    /// Execute end-to-end multiplication.
    Multiply,
    /// Execute IPA in semi-honest majority setting
    SemiHonestIpa(IpaQueryConfig),
    /// Generate inputs for IPA
    GenIpaInputs {
        /// Number of records to generate
        #[clap(long, short = 'n')]
        count: u32,

        /// The destination file for generated records
        #[arg(long)]
        output_file: Option<PathBuf>,

        #[clap(flatten)]
        gen_args: EventGeneratorConfig,
    },
}

#[derive(Debug, clap::Args)]
struct GenInputArgs {
    /// Maximum records per user
    #[clap(long)]
    max_per_user: u32,
    /// number of breakdowns
    breakdowns: u32,
}

async fn clients_ready(clients: &[MpcHelperClient; 3]) -> bool {
    clients[0].echo("").await.is_ok()
        && clients[1].echo("").await.is_ok()
        && clients[2].echo("").await.is_ok()
}

fn validate<I, S>(expected: I, actual: I)
where
    I: IntoIterator<Item = S>,
    I::IntoIter: ExactSizeIterator,
    S: PartialEq + Debug,
{
    let mut expected = expected.into_iter().fuse();
    let mut actual = actual.into_iter().fuse();
    let mut mismatch = Vec::new();

    let mut table = Table::new();
    table.set_header(vec!["Row", "Expected", "Actual", "Diff?"]);

    let mut i = 0;
    loop {
        let next_expected = expected.next();
        let next_actual = actual.next();

        if next_expected.is_none() && next_actual.is_none() {
            break;
        }

        let same = next_expected == next_actual;
        let color = if same { Color::Green } else { Color::Red };
        table.add_row(vec![
            Cell::new(format!("{}", i)).fg(color),
            Cell::new(format!("{:?}", next_expected)).fg(color),
            Cell::new(format!("{:?}", next_actual)).fg(color),
            Cell::new(if same { "" } else { "X" }),
        ]);

        if !same {
            mismatch.push((i, next_expected, next_actual))
        }

        i += 1;
    }

    tracing::info!("\n{table}\n");

    assert!(
        mismatch.is_empty(),
        "Expected and actual results don't match: {:?}",
        mismatch
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let _handle = args.logging.setup_logging();

    let make_clients = || async {
        let scheme = if args.disable_https {
            Scheme::HTTP
        } else {
            Scheme::HTTPS
        };

        let config_path = args.network.as_deref();
        let mut wait = args.wait;

        let config = if let Some(path) = config_path {
            NetworkConfig::from_toml_str(&fs::read_to_string(path).unwrap()).unwrap()
        } else {
            NetworkConfig {
                peers: [
                    PeerConfig::new("localhost:3000".parse().unwrap(), None),
                    PeerConfig::new("localhost:3001".parse().unwrap(), None),
                    PeerConfig::new("localhost:3002".parse().unwrap(), None),
                ],
                client: ClientConfig::default(),
            }
        }
        .override_scheme(&scheme);
        let clients = MpcHelperClient::from_conf(&config, ClientIdentity::None);
        while wait > 0 && !clients_ready(&clients).await {
            tracing::debug!("waiting for servers to come up");
            sleep(Duration::from_secs(1)).await;
            wait -= 1;
        }

        clients
    };

    match args.action {
        TestAction::Multiply => multiply(&args, &make_clients().await).await,
        TestAction::SemiHonestIpa(config) => {
            semi_honest_ipa(&args, &config, &make_clients().await).await
        }
        TestAction::GenIpaInputs {
            count,
            output_file,
            gen_args,
        } => gen_inputs(count, output_file, gen_args).unwrap(),
    };

    Ok(())
}

fn gen_inputs(
    count: u32,
    output_file: Option<PathBuf>,
    args: EventGeneratorConfig,
) -> io::Result<()> {
    let event_gen = EventGenerator::with_config(thread_rng(), args).take(count as usize);
    let mut writer: Box<dyn Write> = if let Some(path) = output_file {
        Box::new(OpenOptions::new().write(true).create_new(true).open(path)?)
    } else {
        Box::new(stdout().lock())
    };

    for event in event_gen {
        event.to_csv(&mut writer)?;
        writer.write(&[b'\n'])?;
    }

    Ok(())
}

async fn semi_honest_ipa(
    args: &Args,
    ipa_query_config: &IpaQueryConfig,
    helper_clients: &[MpcHelperClient; 3],
) {
    let input = InputSource::from(&args.input);
    let query_type = QueryType::Ipa(ipa_query_config.clone());
    let query_config = QueryConfig {
        field_type: args.input.field,
        query_type,
    };
    let query_id = helper_clients[0].create_query(query_config).await.unwrap();
    let input_rows = input.iter::<TestRawDataRecord>().collect::<Vec<_>>();
    let expected = {
        let mut r = ipa_in_the_clear(
            &input_rows,
            ipa_query_config.per_user_credit_cap,
            ipa_query_config.attribution_window_seconds,
        );

        // pad the output vector to the max breakdown key, to make sure it is aligned with the MPC results
        // truncate shouldn't happen unless in_the_clear is badly broken
        r.resize(
            usize::try_from(ipa_query_config.max_breakdown_key).unwrap(),
            0,
        );
        r
    };

    let actual = match args.input.field {
        FieldType::Fp31 => {
            semi_honest::<Fp31, MatchKey, BreakdownKey>(&input_rows, &helper_clients, query_id)
                .await
        }
        FieldType::Fp32BitPrime => {
            semi_honest::<Fp32BitPrime, MatchKey, BreakdownKey>(
                &input_rows,
                &helper_clients,
                query_id,
            )
            .await
        }
    };

    validate(expected, actual)
}

async fn multiply_in_field<F: Field>(
    args: &Args,
    helper_clients: &[MpcHelperClient; 3],
    query_id: QueryId,
) where
    F: Field + IntoShares<AdditiveShare<F>>,
    <F as Serializable>::Size: Add<<F as Serializable>::Size>,
    <<F as Serializable>::Size as Add<<F as Serializable>::Size>>::Output: ArrayLength<u8>,
{
    let input = InputSource::from(&args.input);
    let input_rows: Vec<_> = input.iter::<(F, F)>().collect();
    let expected = input_rows.iter().map(|(a, b)| *a * *b).collect::<Vec<_>>();
    let actual = secure_mul(input_rows, &helper_clients, query_id).await;

    validate(expected, actual);
}

async fn multiply(args: &Args, helper_clients: &[MpcHelperClient; 3]) {
    let query_config = QueryConfig {
        field_type: args.input.field,
        query_type: QueryType::TestMultiply,
    };

    let query_id = helper_clients[0].create_query(query_config).await.unwrap();
    match args.input.field {
        FieldType::Fp31 => multiply_in_field::<Fp31>(args, helper_clients, query_id).await,
        FieldType::Fp32BitPrime => {
            multiply_in_field::<Fp32BitPrime>(args, helper_clients, query_id).await
        }
    };
}

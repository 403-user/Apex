use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "apex",
    about = "Apex Terminal - Next-Gen Kali Linux Offensive Terminal",
    version,
    long_about = concat!(
        "Apex Terminal v", env!("CARGO_PKG_VERSION"), "\n",
        "GPU-accelerated terminal with native multiplexer, AI middleware,\n",
        "and C2 framework connectors for Kali Linux."
    )
)]
struct Cli {
    #[arg(long, default_value = "info", help = "Log level (trace|debug|info|warn|error)")]
    log_level: String,

    #[arg(long, help = "Path to config file")]
    config: Option<String>,

    #[arg(long, help = "Run in server (multiplexer) mode")]
    server: bool,

    #[arg(long, help = "Run in client (GUI) mode")]
    client: bool,
}

fn setup_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("Panic: {}", info);
        hook(info);
    }));
}

fn validate_log_level(level: &str) -> bool {
    matches!(level, "trace" | "debug" | "info" | "warn" | "error")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_panic_hook();

    let cli = Cli::parse();
    if !validate_log_level(&cli.log_level) {
        anyhow::bail!(
            "Invalid log level '{}'. Valid: trace, debug, info, warn, error",
            cli.log_level
        );
    }

    let filter = EnvFilter::new(&cli.log_level)
        .add_directive("wgpu_core=warn".parse().expect("hardcoded directive"))
        .add_directive("wgpu_hal=warn".parse().expect("hardcoded directive"))
        .add_directive("naga=warn".parse().expect("hardcoded directive"))
        .add_directive("cosmic_text=warn".parse().expect("hardcoded directive"))
        .add_directive("glyphon=warn".parse().expect("hardcoded directive"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .init();

    tracing::info!(
        "Apex Terminal v{} initializing (mode: {})",
        env!("CARGO_PKG_VERSION"),
        if cli.server { "server" } else { "client" }
    );

    if cli.server {
        tracing::info!("Starting Apex Terminal server (background multiplexer)");
        apex_server::run_server().await?;
    } else {
        tracing::info!("Starting Apex Terminal client (GUI frontend)");
        apex_config::load_config(cli.config.as_deref())?;
        apex_renderer::run_event_loop().await?;
    }

    Ok(())
}

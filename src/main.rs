use std::path::PathBuf;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod dumb;

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

    #[arg(long, help = "Run in dumb terminal mode (no GPU, raw PTY passthrough)")]
    dumb: bool,

    #[arg(long, help = "Save debug atlas image to PATH (PPM format)")]
    dump_atlas: Option<String>,
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

    if cli.dumb {
        tracing::info!("Starting Apex Terminal in dumb mode (no GPU, raw PTY loop)");
        dumb::run_dumb_terminal().await?;
    } else if cli.server {
        tracing::info!("Starting Apex Terminal server (background multiplexer)");
        apex_server::run_server().await?;
    } else {
        tracing::info!("Starting Apex Terminal client (GUI frontend)");
        let config = apex_config::load_config(cli.config.as_deref())?;
        let atlas_dump = cli.dump_atlas.map(PathBuf::from);
        apex_renderer::run_event_loop(config, atlas_dump).await?;
    }

    Ok(())
}

mod registered_modules;

use anyhow::Result;
use clap::{Parser, Subcommand};
use mimalloc::MiMalloc;
use modkit::bootstrap::{
    AppConfig, dump_effective_modules_config_json, dump_effective_modules_config_yaml,
    list_module_names, run_migrate, run_server,
};

use std::path::PathBuf;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// `HyperSpot` Server - modular platform for AI services
#[derive(Parser)]
#[command(name = "hyperspot-server")]
#[command(about = "HyperSpot Server - modular platform for AI services")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Port override for HTTP server (overrides config)
    #[arg(short, long)]
    port: Option<u16>,

    /// Print effective configuration (YAML) and exit
    #[arg(long)]
    print_config: bool,

    /// List all configured module names and exit
    #[arg(long)]
    list_modules: bool,

    /// Dump effective per-module configuration (YAML) and exit
    #[arg(long)]
    dump_modules_config_yaml: bool,

    /// Dump effective per-module configuration (JSON) and exit
    #[arg(long)]
    dump_modules_config_json: bool,

    /// Log verbosity level (-v debug, -vv trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the server
    Run,
    /// Do nothing
    Check,
    /// Run database migrations and exit (for cloud deployments)
    Migrate,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install aws-lc-rs as the default rustls CryptoProvider before any TLS
    // config is constructed. Required because both `ring` and `aws-lc-rs`
    // providers are compiled in (ring via aliri/pingora-rustls), and rustls
    // 0.23 panics if it cannot auto-detect a single provider.
    // Skip in FIPS mode — init_procedure() installs the FIPS provider instead.
    #[cfg(not(feature = "fips"))]
    if let Err(_e) = rustls::crypto::aws_lc_rs::default_provider().install_default() {
        // Another provider was already installed — safe to continue.
    }

    let cli = Cli::parse();

    // Layered config:
    // 1) defaults -> 2) YAML (if provided) -> 3) env (APP__*) -> 4) CLI overrides
    // Also normalizes + creates server.home_dir.
    let mut config = AppConfig::load_or_default(cli.config.as_ref())?;
    config.apply_cli_overrides(cli.verbose);

    // Print config and exit if requested
    if cli.print_config {
        println!("Effective configuration:\n{}", config.to_yaml()?);
        return Ok(());
    }

    // List all configured modules and exit if requested
    if cli.list_modules {
        let modules = list_module_names(&config);
        println!("Configured modules ({}):", modules.len());
        for module in modules {
            println!("  - {module}");
        }
        return Ok(());
    }

    // Dump modules config in YAML format and exit if requested
    if cli.dump_modules_config_yaml {
        let yaml = dump_effective_modules_config_yaml(&config)?;
        println!("{yaml}");
        return Ok(());
    }

    // Dump modules config in JSON format and exit if requested
    if cli.dump_modules_config_json {
        let json = dump_effective_modules_config_json(&config)?;
        println!("{json}");
        return Ok(());
    }

    // Dispatch subcommands (default: run)
    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => run_server(config).await,
        Commands::Migrate => run_migrate(config).await,
        Commands::Check => Ok(()),
    }
}

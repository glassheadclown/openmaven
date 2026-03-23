mod backfill;
mod config;
mod diagnostics;
mod errors;
mod export;
mod narrative;
mod notify;
mod poller;
mod sentiment;
mod store;
mod trends;
mod tui_dashboard;
mod web;
mod wizard;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use backfill::{Backfill, BackfillTarget, PostSort};
use errors::new_provider_health;
use export::{ExportFormat, Exporter};
use narrative::spawn_narrative_schedulers;
use trends::TrendTracker;
use tui_dashboard::LiveDashboard;

#[derive(Parser)]
#[command(
    name = "openmaven",
    about = "Reddit intelligence platform",
    version = env!("CARGO_PKG_VERSION"),
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the live polling daemon (default)
    Run {
        #[arg(long, short)]
        dashboard: bool,
    },
    /// Open the live TUI dashboard
    Dashboard,
    /// Re-run setup wizard
    Setup,
    /// Backfill historical posts from a subreddit
    Backfill {
        #[arg(long, short)]
        subreddit: Option<String>,
        #[arg(long)]
        submission: Option<String>,
        #[arg(long, default_value = "hot")]
        sort: String,
        #[arg(long, default_value = "10")]
        limit: u32,
        #[arg(long, default_value = "")]
        keywords: String,
    },
    /// Export stored results to CSV / JSON / Markdown
    Export {
        #[arg(long, short, value_enum, default_value = "markdown")]
        format: ExportFormat,
        #[arg(long, short, default_value = ".")]
        output: PathBuf,
        #[arg(long, default_value = "1000")]
        limit: i64,
    },
    /// Print stats from local database
    Stats,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "openmaven=info,warn".to_string()),
        )
        .without_time()
        .init();

    if let Some(path) = &cli.config {
        std::env::set_var("OPENMAVEN_CONFIG", path);
    }

    let command = cli.command.unwrap_or(Command::Run { dashboard: false });

    match command {
        Command::Setup => {
            run_wizard(true).await?;
        }

        Command::Run { dashboard } => {
            let mut cfg = ensure_config().await?;
            let store = Arc::new(store::Store::connect().await?);
            let health = new_provider_health();
            let tracker = TrendTracker::new(900, 60);

            if cfg.web.enabled {
                let store_clone = Arc::clone(&store);
                let port = cfg.web.port;
                tokio::spawn(async move {
                    if let Err(e) = web::serve(store_clone, port).await {
                        tracing::error!("Web server error: {}", e);
                    }
                });
                info!("Dashboard → http://localhost:{}", port);
            }

            if dashboard {
                let store_clone = Arc::clone(&store);
                let tracker_clone = Arc::clone(&tracker);
                tokio::spawn(async move {
                    let dash = LiveDashboard::new(store_clone, tracker_clone);
                    if let Err(e) = dash.run().await {
                        tracing::error!("TUI error: {}", e);
                    }
                });
            }

            let tracker_snap = Arc::clone(&tracker);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    tracker_snap.snapshot_all().await;
                }
            });

            // Migrate DB — must happen before narrative engine spawns
            store.migrate_narratives().await?;

            // Narrative engine schedulers
            let notifier = Arc::new(notify::Notifier::new(cfg.notifications.clone()));

            if cfg.analysis.mode != config::AnalysisMode::Raw {
                spawn_narrative_schedulers(
                    cfg.tracking.subreddits.clone(),
                    Arc::clone(&store),
                    cfg.ai.clone(),
                    cfg.analysis.clone(),
                    Arc::clone(&notifier),
                    Arc::clone(&health),
                );
            }

            // First-run diagnostics — sent once, then flagged in config
            diagnostics::send_first_run_report(&mut cfg, Arc::clone(&notifier)).await;

            info!("openmaven running. Ctrl+C to stop.");
            let p = poller::Poller::new(cfg, store, tracker, health);
            p.run().await?;
        }

        Command::Dashboard => {
            ensure_config().await?;
            let store = Arc::new(store::Store::connect().await?);
            let tracker = TrendTracker::new(900, 60);
            let dash = LiveDashboard::new(store, tracker);
            dash.run().await?;
        }

        Command::Backfill { subreddit, submission, sort, limit, keywords } => {
            let cfg = ensure_config().await?;
            let store = Arc::new(store::Store::connect().await?);

            let target = if let Some(sub) = subreddit {
                let sort_mode = match sort.as_str() {
                    "new" => PostSort::New,
                    "top" => PostSort::Top,
                    "rising" => PostSort::Rising,
                    _ => PostSort::Hot,
                };
                BackfillTarget::Subreddit { name: sub, sort: sort_mode, limit }
            } else if let Some(id) = submission {
                BackfillTarget::Submission { id }
            } else {
                anyhow::bail!("Provide --subreddit or --submission");
            };

            let kws: Vec<String> = keywords
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let bf = Backfill::new(cfg, store);
            let count = bf.run(target, &kws).await?;
            println!("\n  ✓ Backfill complete — {} comments analyzed.\n", count);
        }

        Command::Export { format, output, limit } => {
            ensure_config().await?;
            let store = store::Store::connect().await?;
            let results = store.recent(limit).await?;
            let stats = store.stats().await?;
            let path = Exporter::export_results(&results, &stats, &[], format, &output)?;
            println!("\n  ✓ Exported → {}\n", path.display());
        }

        Command::Stats => {
            ensure_config().await?;
            let store = store::Store::connect().await?;
            let stats = store.stats().await?;
            let recent = store.recent(10_000).await?;

            println!("\n  openmaven stats\n");
            println!("  {:<25} {:>8} {:>10} {:>10} {:>10}", "Subreddit", "Total", "Avg", "Pos%", "Neg%");
            println!("  {}", "─".repeat(68));
            for s in &stats {
                println!(
                    "  {:<25} {:>8} {:>10.3} {:>9}% {:>9}%",
                    format!("r/{}", s.subreddit),
                    s.total,
                    s.avg_score,
                    if s.total > 0 { s.positive_count * 100 / s.total } else { 0 },
                    if s.total > 0 { s.negative_count * 100 / s.total } else { 0 },
                );
            }

            let total = recent.len();
            let avg = if total > 0 {
                recent.iter().map(|r| r.score as f64).sum::<f64>() / total as f64
            } else { 0.0 };
            println!("\n  Total: {}  |  Overall avg: {:+.3}\n", total, avg);
        }
    }

    Ok(())
}

async fn ensure_config() -> Result<config::Config> {
    if !config::Config::exists() {
        println!("No config found. Running setup...\n");
        return run_wizard(false).await;
    }
    config::Config::load()
}

async fn run_wizard(force: bool) -> Result<config::Config> {
    if config::Config::exists() && !force {
        return config::Config::load();
    }
    let mut w = wizard::Wizard::new();
    match w.run()? {
        Some(cfg) => {
            cfg.save()?;
            println!("\n  Config saved → {:?}\n", config::config_path());
            Ok(cfg)
        }
        None => anyhow::bail!("Setup cancelled."),
    }
}

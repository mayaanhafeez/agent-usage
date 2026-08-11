mod detect;
mod model;
mod ui;
mod usage;

use anyhow::Result;
use clap::Parser;

use model::Provider;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Override automatic agent detection
    #[arg(short, long, value_name = "AGENT")]
    provider: Option<Provider>,

    /// Print snapshots as line-oriented text instead of opening the TUI
    #[arg(long)]
    plain: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let providers = args
        .provider
        .map(|provider| vec![provider])
        .unwrap_or_else(detect::detect_all);
    anyhow::ensure!(
        !providers.is_empty(),
        "no running agent detected; use --provider claude|codex|opencode|gemini"
    );
    let snapshots = usage::collect_all(&providers);
    if args.plain {
        for snapshot in snapshots {
            print_snapshot(snapshot);
        }
        return Ok(());
    }
    ui::run(snapshots)
}

fn print_snapshot(snapshot: model::Snapshot) {
    println!(
        "agent={} plan={}",
        snapshot.provider.name(),
        snapshot.plan.as_deref().unwrap_or("unknown")
    );
    for limit in &snapshot.limits {
        println!(
            "limit={} used={} reset={}",
            limit.label,
            limit.percent,
            limit.resets.as_deref().unwrap_or("unknown")
        );
    }
    for day in &snapshot.days {
        println!("day={} tokens={}", day.label, day.tokens);
    }
    for model in &snapshot.models {
        println!("model={} tokens={}", model.label, model.tokens);
    }
    if let Some(note) = snapshot.note {
        println!("note={note}");
    }
}

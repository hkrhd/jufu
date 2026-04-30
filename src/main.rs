mod app;
mod jj;
mod model;
mod ui;

use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event;

use crate::app::{App, BackgroundEvent, CommandSender, ControlFlow, Effect};
use crate::model::AppConfig;

#[derive(Debug, Parser)]
#[command(version, about = "Jujutsu log viewer TUI inspired by keifu")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = Cli::parse();

    let cwd = std::env::current_dir()?;
    jj::ensure_jj_available(&cwd).await?;
    let config = AppConfig { repo_path: cwd };

    let mut terminal = ui::init_terminal()?;
    let result = run(&mut terminal, config).await;
    let restore_result = ui::restore_terminal();
    restore_result?;
    result
}

async fn run(terminal: &mut ui::Terminal, config: AppConfig) -> Result<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<BackgroundEvent>();
    let mut app = App::new(config);

    run_effects(
        &sender,
        app.config().repo_path.clone(),
        app.startup_effects(),
    );

    loop {
        while let Ok(message) = receiver.try_recv() {
            let effects = app.apply_background_event(message);
            run_effects(&sender, app.config().repo_path.clone(), effects);
        }

        terminal.draw(|frame| ui::render(frame, &mut app))?;

        if event::poll(Duration::from_millis(100))? {
            let event = event::read()?;
            let update = app.handle_event(event);
            run_effects(&sender, app.config().repo_path.clone(), update.effects);
            if matches!(update.control_flow, ControlFlow::Exit) {
                break;
            }
        }
    }

    Ok(())
}

fn run_effects(sender: &CommandSender, repo_path: std::path::PathBuf, effects: Vec<Effect>) {
    for effect in effects {
        spawn_effect(sender, repo_path.clone(), effect);
    }
}

fn spawn_effect(sender: &CommandSender, repo_path: std::path::PathBuf, effect: Effect) {
    let tx = sender.clone();

    match effect {
        Effect::LoadLogs => {
            tokio::spawn(async move {
                let result = jj::load_logs(&repo_path).await;
                let _ = tx.send(BackgroundEvent::LogsLoaded(result));
            });
        }
        Effect::LoadDiff { change_id } => {
            tokio::spawn(async move {
                let result = jj::load_diff_stat(&repo_path, &change_id).await;
                let _ = tx.send(BackgroundEvent::DiffLoaded { change_id, result });
            });
        }
    }
}

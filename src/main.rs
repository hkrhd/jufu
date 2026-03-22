mod app;
mod jj;
mod model;
mod ui;

use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event;

use crate::app::{App, BackgroundEvent, CommandSender, ControlFlow};
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

    spawn_log_load(&sender, app.config().repo_path.clone());

    loop {
        while let Ok(message) = receiver.try_recv() {
            app.apply_background_event(message, &sender);
        }

        terminal.draw(|frame| ui::render(frame, &mut app))?;

        if event::poll(Duration::from_millis(100))? {
            let event = event::read()?;
            if matches!(app.handle_event(event, &sender), ControlFlow::Exit) {
                break;
            }
        }
    }

    Ok(())
}

fn spawn_log_load(sender: &CommandSender, repo_path: std::path::PathBuf) {
    let tx = sender.clone();
    tokio::spawn(async move {
        let result = jj::load_logs(&repo_path).await;
        let _ = tx.send(BackgroundEvent::LogsLoaded(result));
    });
}

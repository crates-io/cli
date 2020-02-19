use crate::{error::Result, model, persistence::Db, utils::*};
use futures::{
    executor::{block_on, ThreadPool},
    future::Either,
    future::FutureExt,
    stream::StreamExt,
    task::{Spawn, SpawnExt},
};
use log::{info, warn};
use prodash::tui::{Event, Line};
use std::{
    io::Write,
    path::Path,
    path::PathBuf,
    time::{Duration, SystemTime},
};

mod changes;
mod worker;

pub struct Context {
    db: Db,
    progress: prodash::tree::Item,
    deadline: Option<SystemTime>,
    download: async_std::sync::Sender<worker::DownloadTask>,
}

/// Runs the statistics and mining engine.
/// May run for a long time unless a deadline is specified.
/// Even though timeouts can be achieved from outside of the future, knowing the deadline may be used
/// by the engine to manage its time even more efficiently.
pub async fn run(
    db: Db,
    crates_io_path: PathBuf,
    deadline: Option<SystemTime>,
    progress: prodash::Tree,
    pool: impl Spawn,
) -> Result<()> {
    check(deadline)?;

    let mut downloaders = progress.add_child("Downloads");
    let (tx, rx) = async_std::sync::channel(1);
    for idx in 0..10 {
        pool.spawn(worker::download(
            downloaders.add_child(format!("DL {} - idle", idx + 1)),
            rx.clone(),
        ))?;
    }

    let interval_s = 60;
    async move {
        loop {
            let db = db.clone();
            changes::process(
                crates_io_path.clone(),
                &pool,
                Context {
                    db,
                    progress: progress.add_child("crates.io refresh"),
                    deadline,
                    download: tx.clone(),
                },
            )
            .await?;
            let mut fetch_interval_progress = progress.add_child("Fetch Timer");
            wait_with_progress(interval_s, &mut fetch_interval_progress, deadline).await?;
        }
    }
    .await
}

/// For convenience, run the engine and block until done.
pub fn run_blocking(
    db: impl AsRef<Path>,
    crates_io_path: impl AsRef<Path>,
    deadline: Option<SystemTime>,
) -> Result<()> {
    let start_of_computation = SystemTime::now();
    // NOTE: pool should be big enough to hold all possible blocking tasks running in parallel, +1 for
    // additional non-blocking tasks.
    // The main thread is expected to pool non-blocking tasks.
    // I admit I don't fully understand why multi-pool setups aren't making progress… . So just one pool for now.
    let pool_size = 1 + 1;
    let task_pool = ThreadPool::builder().pool_size(pool_size).create()?;
    let db = Db::open(db)?;

    let root = prodash::Tree::new();
    let (gui, abort_handle) = futures::future::abortable(prodash::tui::render_with_input(
        root.clone(),
        prodash::tui::TuiOptions {
            title: "Criner".into(),
            ..prodash::tui::TuiOptions::default()
        },
        context_stream(&db, start_of_computation),
    )?);

    // dropping the work handle will stop (non-blocking) futures
    let work_handle = task_pool.spawn_with_handle(run(
        db.clone(),
        crates_io_path.as_ref().into(),
        deadline,
        root,
        task_pool.clone(),
    ))?;

    let either = block_on(futures::future::select(work_handle, gui.boxed_local()));
    match either {
        Either::Left((work_result, gui)) => {
            abort_handle.abort();
            block_on(gui).ok();
            if let Err(e) = work_result {
                warn!("{}", e);
            }
        }
        Either::Right((_, work_handle)) => work_handle.forget(),
    }

    // Make sure the terminal can reset when the gui is done.
    std::io::stdout().flush()?;

    // at this point, we forget all currently running computation, and since it's in the local thread, it's all
    // destroyed/dropped properly.
    info!("{}", wallclock(start_of_computation));
    Ok(())
}

fn wallclock(since: SystemTime) -> String {
    format!(
        "Wallclock elapsed: {}",
        humantime::format_duration(SystemTime::now().duration_since(since).unwrap_or_default())
    )
}

fn context_stream(db: &Db, start_of_computation: SystemTime) -> impl futures::Stream<Item = Event> {
    prodash::tui::ticker(Duration::from_secs(1)).map({
        let db = db.clone();
        move |_| {
            db.context()
                .iter()
                .next_back()
                .and_then(Result::ok)
                .map(|(_, c): (_, model::Context)| {
                    let lines = vec![
                        Line::Text(wallclock(start_of_computation)),
                        Line::Title("Durations".into()),
                        Line::Text(format!(
                            "fetch-crate-versions: {:?}",
                            c.durations.fetch_crate_versions
                        )),
                        Line::Title("Counts".into()),
                        Line::Text(format!("crate-versions: {}", c.counts.crate_versions)),
                        Line::Text(format!("        crates: {}", c.counts.crates)),
                    ];
                    Event::SetInformation(lines)
                })
                .unwrap_or(Event::Tick)
        }
    })
}

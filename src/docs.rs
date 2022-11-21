use std::{path::Path, thread};

use tokio::sync::mpsc::UnboundedReceiver;
use tracing::info;

use crate::crate_path;

pub fn start_background_thread(
    data_dir: impl AsRef<Path>,
    mut work_queue: UnboundedReceiver<(String, String)>,
) {
    let data_dir = data_dir.as_ref().to_owned();
    thread::spawn(move || loop {
        let (crate_name, crate_version) = work_queue.blocking_recv().unwrap();
        info!("building docs for {crate_name}@{crate_version}");
        let meta = std::fs::metadata(crate_path(&data_dir, &crate_name, &crate_version)).unwrap();
    });
}

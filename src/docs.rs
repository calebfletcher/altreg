use std::{fs, io, path::Path, thread};

use rustwide::{cmd::SandboxBuilder, Crate, Toolchain, WorkspaceBuilder};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::info;

pub fn start_background_thread(
    data_dir: impl AsRef<Path>,
    mut work_queue: UnboundedReceiver<(String, String)>,
) {
    let data_dir = data_dir.as_ref().to_owned();
    thread::spawn(move || {
        info!("preparing docs build environment");
        // Create a new workspace in .workspaces/docs-builder
        let workspace =
            WorkspaceBuilder::new(Path::new(".workspaces/docs-builder"), "altreg-docs-builder")
                .init()
                .unwrap();
        workspace.purge_all_build_dirs().unwrap();

        // Run the builds on stable
        let toolchain = Toolchain::dist("stable");
        toolchain.install(&workspace).unwrap();

        info!("docs builder ready");

        loop {
            let (name, version) = work_queue.blocking_recv().unwrap();
            info!("building docs for {name}@{version}");

            // Configure a sandbox with 1GB of RAM and no network access
            let sandbox = SandboxBuilder::new()
                .memory_limit(Some(1024 * 1024 * 1024))
                .enable_networking(false);

            // Create a build directory for this build
            let mut build_dir = workspace.build_dir(&format!("{}-{}", name, version));
            build_dir.purge().unwrap();

            // Fetch lazy_static from crates.io
            let krate = Crate::crates_io("lazy_static", "1.0.0");
            krate.fetch(&workspace).unwrap();

            info!("building crate docs");
            build_dir
                .build(&toolchain, &krate, sandbox)
                .run(|build| {
                    // Build docs
                    build
                        .cargo()
                        .args(&["doc", "--offline", "--no-deps"])
                        .run()?;

                    // Copy docs to data directory
                    let source_dir = build.host_target_dir().join("doc");
                    let dest_dir = data_dir.join("docs").join(name).join(version);
                    copy_dir_all(source_dir, dest_dir).unwrap();

                    Ok(())
                })
                .unwrap();

            // Clean up
            build_dir.purge().unwrap();
            krate.purge_from_cache(&workspace).unwrap();
            info!("built crate");
        }
    });
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    let dst = dst.as_ref();
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let filename = entry.file_name();
        if entry.file_type()?.is_dir() {
            copy_dir_all(entry.path(), dst.join(filename))?;
        } else {
            fs::copy(entry.path(), dst.join(filename))?;
        }
    }
    Ok(())
}

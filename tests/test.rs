use tempfile::tempdir;

use file_per_thread_logger::initialize;

use log::{debug, error, info, trace, warn};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::thread;

const LOG_PREFIX: &str = "my_log_test-";

fn log_files() -> io::Result<HashSet<String>> {
    let mut logs = HashSet::new();
    let current_dir = env::current_dir()?;
    for entry in fs::read_dir(current_dir.as_path())? {
        let path = entry?.path();
        if let Some(filename) = path.file_name() {
            let filename = filename.to_string_lossy();
            if filename.starts_with(LOG_PREFIX) {
                logs.insert(filename[LOG_PREFIX.len()..].to_string());
            }
        }
    }
    Ok(logs)
}

fn read_log(name: &str) -> io::Result<String> {
    fs::read_to_string(format!("{}{}", LOG_PREFIX, name))
}

fn set(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn flush() {
    log::logger().flush();
}

fn do_log(run_init: bool) -> thread::ThreadId {
    trace!("This is a trace entry on the main thread.");
    debug!("This is a debug entry on the main thread.");
    info!("This is an info entry on the main thread.");
    warn!("This is a warn entry on the main thread.");
    error!("This is an error entry on the main thread.");

    let handle = thread::spawn(move || {
        if run_init {
            initialize(LOG_PREFIX);
        }
        trace!("This is a trace entry from an unnamed helper thread.");
        debug!("This is a debug entry from an unnamed helper thread.");
        info!("This is an info entry from an unnamed helper thread.");
        warn!("This is a warn entry from an unnamed helper thread.");
        error!("This is an error entry from an unnamed helper thread.");
        flush();
    });

    let unnamed_thread_id = handle.thread().id();
    handle.join().unwrap();

    let handle = thread::Builder::new()
        .name("helper".to_string())
        .spawn(move || {
            if run_init {
                initialize(LOG_PREFIX);
            }
            trace!("This is a trace entry from a named thread.");
            debug!("This is a debug entry from a named thread.");
            info!("This is an info entry from a named thread.");
            warn!("This is a warn entry from a named thread.");
            error!("This is an error entry from a named thread.");
            flush();
        })
        .unwrap();

    handle.join().unwrap();
    flush();

    unnamed_thread_id
}

#[test]
fn tests() -> io::Result<()> {
    let temp_dir = tempdir()?;
    env::set_current_dir(&temp_dir)?;

    assert_eq!(log_files()?, set(&[]));

    env::remove_var("RUST_LOG");
    initialize(LOG_PREFIX);

    // Nothing should be logged without something in the RUST_LOG env variable..
    assert_eq!(log_files()?, set(&[]));
    do_log(false);
    assert_eq!(log_files()?, set(&[]));

    // When the RUST_LOG variable is set, it will create the main thread file even though nothing
    // has been logged yet.
    env::set_var("RUST_LOG", "info");
    initialize(LOG_PREFIX);
    flush();

    let main_log = "tests";
    let named_log = "helper";

    assert_eq!(log_files()?, set(&[main_log]));
    assert_eq!(
        read_log(main_log)?,
        r#"INFO - Set up logging; filename prefix is my_log_test-
"#
    );

    let unnamed_thread_id = do_log(true);
    let unnamed_log = format!("{:?}", unnamed_thread_id);
    let unnamed_log = &unnamed_log
        .chars()
        .filter(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();

    // It then creates files for each thread with logged contents.
    assert_eq!(log_files()?, set(&[main_log, named_log, unnamed_log]));
    assert_eq!(
        read_log(main_log)?,
        r#"INFO - Set up logging; filename prefix is my_log_test-
INFO - This is an info entry on the main thread.
WARN - This is a warn entry on the main thread.
ERROR - This is an error entry on the main thread.
"#
    );
    assert_eq!(
        read_log(unnamed_log)?,
        r#"INFO - Set up logging; filename prefix is my_log_test-
INFO - This is an info entry from an unnamed helper thread.
WARN - This is a warn entry from an unnamed helper thread.
ERROR - This is an error entry from an unnamed helper thread.
"#
    );
    assert_eq!(
        read_log(named_log)?,
        r#"INFO - Set up logging; filename prefix is my_log_test-
INFO - This is an info entry from a named thread.
WARN - This is a warn entry from a named thread.
ERROR - This is an error entry from a named thread.
"#
    );

    temp_dir.close()?;
    Ok(())
}

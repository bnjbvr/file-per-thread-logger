use tempfile::tempdir;

use file_per_thread_logger::initialize;

use log::{ trace, debug, info, warn, error };
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::env;

fn no_log_file_exists() -> io::Result<bool> {
    let current_dir = env::current_dir()?;
    for entry in fs::read_dir(current_dir.as_path())? {
        let path = entry?.path();
        if let Some(filename) = path.file_name() {
            if filename.to_string_lossy().starts_with("my_log_test-") {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn flush() {
    log::logger().flush();
}

fn do_log(run_init: bool) {
    trace!("This is a trace entry on the main thread.");
    debug!("This is a debug entry on the main thread.");
    info!("This is an info entry on the main thread.");
    warn!("This is a warn entry on the main thread.");
    error!("This is an error entry on the main thread.");

    let handle = thread::spawn(move || {
        if run_init {
            initialize("my_log_test-");
        }
        trace!("This is a trace entry from an unnamed helper thread.");
        debug!("This is a debug entry from an unnamed helper thread.");
        info!("This is an info entry from an unnamed helper thread.");
        warn!("This is a warn entry from an unnamed helper thread.");
        error!("This is an error entry from an unnamed helper thread.");
        flush();
    });

    handle.join().unwrap();

    let handle = thread::Builder::new().name("helper".to_string()).spawn(move || {
        if run_init {
            initialize("my_log_test-");
        }
        trace!("This is a trace entry from a named thread.");
        debug!("This is a debug entry from a named thread.");
        info!("This is an info entry from a named thread.");
        warn!("This is a warn entry from a named thread.");
        error!("This is an error entry from a named thread.");
        flush();
    }).unwrap();

    handle.join().unwrap();
    flush();
}

#[test]
fn tests() -> io::Result<()> {
    let temp_dir = tempdir()?;
    env::set_current_dir(&temp_dir)?;

    assert!(no_log_file_exists()?);

    env::remove_var("RUST_LOG");
    initialize("my_log_test-");

    // Nothing should be logged without something in the RUST_LOG env variable..
    assert!(no_log_file_exists()?);
    do_log(false);
    assert!(no_log_file_exists()?);

    // When the RUST_LOG variable is set, it will create the main thread file even though nothing
    // has been logged yet.
    env::set_var("RUST_LOG", "info");
    initialize("my_log_test-");
    flush();

    let main_log = Path::new("my_log_test-tests");
    let named_log = Path::new("my_log_test-helper");
    // do_log spawns 2 threads. This is the second time we call do_log, and we start counting from
    // 1 (main thread). So the second unnamed thread has id = 4.
    let unnamed_log = Path::new("my_log_test-ThreadId4");

    assert!(main_log.exists());
    assert_eq!(fs::read_to_string(main_log)?, r#"INFO - Set up logging; filename prefix is my_log_test-
"#);

    assert!(!unnamed_log.exists());
    assert!(!named_log.exists());

    do_log(true);

    // It then creates files for each thread with logged contents.
    assert!(main_log.exists());
    assert_eq!(fs::read_to_string(main_log)?, r#"INFO - Set up logging; filename prefix is my_log_test-
INFO - This is an info entry on the main thread.
WARN - This is a warn entry on the main thread.
ERROR - This is an error entry on the main thread.
"#);

    assert!(unnamed_log.exists());
    assert_eq!(fs::read_to_string(unnamed_log)?, r#"INFO - Set up logging; filename prefix is my_log_test-
INFO - This is an info entry from an unnamed helper thread.
WARN - This is a warn entry from an unnamed helper thread.
ERROR - This is an error entry from an unnamed helper thread.
"#);

    assert!(named_log.exists());
    assert_eq!(fs::read_to_string(named_log)?, r#"INFO - Set up logging; filename prefix is my_log_test-
INFO - This is an info entry from a named thread.
WARN - This is a warn entry from a named thread.
ERROR - This is an error entry from a named thread.
"#);

    temp_dir.close()?;
    Ok(())
}

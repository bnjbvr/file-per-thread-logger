use std::cell::{RefCell, RefMut};
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use env_logger::{Builder, Logger};
use log::{LevelFilter, Metadata, Record};

thread_local! {
    static WRITER: RefCell<Option<io::BufWriter<File>>> = RefCell::new(None);
}

static ALLOW_UNINITIALIZED: AtomicBool = AtomicBool::new(false);

/// Helper struct that can help retrieve a writer, from within a custom format function.
///
/// Use `GetWriter::get()` to retrieve an instance of the writer.
pub struct GetWriter<'a> {
    rc: &'a RefCell<Option<io::BufWriter<File>>>,
}

impl<'a> GetWriter<'a> {
    /// Retrieves a mutable reference to the underlying buffer writer.
    pub fn get(&self) -> RefMut<'a, io::BufWriter<File>> {
        RefMut::map(self.rc.borrow_mut(), |maybe_buf_writer| {
            maybe_buf_writer
                .as_mut()
                .expect("call the logger's initialize() function first")
        })
    }
}

/// Format function to print logs in a custom format.
///
/// Note: to allow for reentrant log invocations, `record.args()` must be reified before the writer
/// has been taken with the `GetWriter` instance, otherwise double borrows runtime panics may
/// occur.
pub type FormatFn = fn(&GetWriter, &Record) -> io::Result<()>;

/// Initializes the current process/thread with a logger, parsing the RUST_LOG environment
/// variables to set the logging level filter and/or directives to set a filter by module name,
/// following the usual env_logger conventions.
///
/// Must be called on every running thread, or else logging will panic the first time it's used.
/// ```
/// use file_per_thread_logger::initialize;
///
/// initialize("log-file-prefix");
/// ```
pub fn initialize(filename_prefix: &str) {
    init_logging(filename_prefix, None)
}

/// Initializes the current process/thread with a logger, parsing the RUST_LOG environment
/// variables to set the logging level filter and/or directives to set a filter by module name,
/// following the usual env_logger conventions. The format function specifies the format in which
/// the logs will be printed.
///
/// To allow for recursive log invocations (a log happening in an argument to log), the format
/// function must take care of reifying the record's argument *before* taking the reference to the
/// writer, at the risk of causing double-borrows runtime panics otherwise.
///
/// Must be called on every running thread, or else logging will panic the first time it's used.
/// ```
/// use file_per_thread_logger::{initialize_with_formatter, FormatFn};
/// use std::io::Write;
///
/// let formatter: FormatFn = |writer, record| {
///     // Reify arguments first, to allow for recursive log invocations.
///     let args = format!("{}", record.args());
///     writeln!(
///         writer,
///         "{} [{}:{}] {}",
///         record.level(),
///         record.file().unwrap_or_default(),
///         record.line().unwrap_or_default(),
///         args,
///     )
/// };
/// initialize_with_formatter("log-file-prefix", formatter);
/// ```
pub fn initialize_with_formatter(filename_prefix: &str, formatter: FormatFn) {
    init_logging(filename_prefix, Some(formatter))
}

/// Allow logs files to be created from threads in which the logger is specifically uninitialized.
/// It can be useful when you don't have control on threads spawned by a dependency, for instance.
///
/// Should be called before calling code that spawns the new threads.
pub fn allow_uninitialized() {
    ALLOW_UNINITIALIZED.store(true, Ordering::Relaxed);
}

fn init_logging(filename_prefix: &str, formatter: Option<FormatFn>) {
    let env_var = env::var_os("RUST_LOG");
    if env_var.is_none() {
        return;
    }

    let logger = {
        let mut builder = Builder::new();
        builder.parse_filters(env_var.unwrap().to_str().unwrap());
        builder.build()
    };

    // Ensure the thread local state is always properly initialized.
    WRITER.with(|rc| {
        if rc.borrow().is_none() {
            rc.replace(Some(open_file(filename_prefix)));
        }
    });

    let logger = FilePerThreadLogger::new(logger, formatter);
    let _ =
        log::set_boxed_logger(Box::new(logger)).map(|()| log::set_max_level(LevelFilter::max()));

    log::info!("Set up logging; filename prefix is {}", filename_prefix);
}

struct FilePerThreadLogger {
    logger: Logger,
    formatter: Option<FormatFn>,
}

impl FilePerThreadLogger {
    pub fn new(logger: Logger, formatter: Option<FormatFn>) -> Self {
        FilePerThreadLogger { logger, formatter }
    }
}

impl log::Log for FilePerThreadLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.logger.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        WRITER.with(|rc| {
            if ALLOW_UNINITIALIZED.load(Ordering::Relaxed) {
                // Initialize the logger with a default value, if it's not done yet.
                let mut rc = rc.borrow_mut();
                if rc.is_none() {
                    *rc = Some(open_file(""));
                }
            }

            if let Some(ref format_fn) = self.formatter {
                let get_writer = GetWriter { rc };
                let _ = format_fn(&get_writer, record);
            } else {
                // A note: we reify the argument first, before taking a hold on the mutable
                // refcell, in case reifing args will cause a reentrant log invocation. Otherwise,
                // we'd end up with a double borrow of the refcell.
                let args = format!("{}", record.args());

                let mut opt_writer = rc.borrow_mut();
                let writer = opt_writer
                    .as_mut()
                    .expect("call the logger's initialize() function first");

                let _ = writeln!(*writer, "{} - {}", record.level(), args);
            }
        })
    }

    fn flush(&self) {
        WRITER.with(|rc| {
            let mut opt_writer = rc.borrow_mut();
            let writer = opt_writer
                .as_mut()
                .expect("call the logger's initialize() function first");
            let _ = writer.flush();
        });
    }
}

/// Open the tracing file for the current thread.
fn open_file(filename_prefix: &str) -> io::BufWriter<File> {
    let curthread = thread::current();
    let tmpstr;
    let mut path = filename_prefix.to_owned();
    path.extend(
        match curthread.name() {
            Some(name) => name.chars(),
            // The thread is unnamed, so use the thread ID instead.
            None => {
                tmpstr = format!("{:?}", curthread.id());
                tmpstr.chars()
            }
        }
        .filter(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_'),
    );
    let file = File::create(path).expect("Can't open tracing file");
    io::BufWriter::new(file)
}

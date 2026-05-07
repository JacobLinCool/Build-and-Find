use clap::{Parser, ValueEnum};
use minidb::cli::{run_eval, run_file, run_repl, OutputFormat, Session};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(
    name = "minidb",
    version,
    about = "An embeddable SQL-subset relational database"
)]
struct Cli {
    /// Path to the database file
    #[arg(long, short = 'd', default_value = "./minidb.db")]
    db: PathBuf,

    /// Execute a SQL statement and exit
    #[arg(long, short = 'e', conflicts_with = "file")]
    eval: Option<String>,

    /// Execute a SQL script file and exit
    #[arg(long, short = 'f', conflicts_with = "eval")]
    file: Option<PathBuf>,

    /// Force interactive REPL mode
    #[arg(long)]
    repl: bool,

    /// Continue executing on error (for --file and --eval)
    #[arg(long)]
    continue_on_error: bool,

    /// Suppress per-statement summary output
    #[arg(long, short = 'q')]
    quiet: bool,

    /// Output format for SELECT results
    #[arg(long, value_enum, default_value = "table")]
    format: FormatArg,

    /// Initialize a fresh empty database file and exit
    #[arg(long)]
    init: bool,
}

#[derive(Debug, Clone, ValueEnum)]
enum FormatArg {
    Table,
    Csv,
    Json,
}

impl From<FormatArg> for OutputFormat {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Table => OutputFormat::Table,
            FormatArg::Csv => OutputFormat::Csv,
            FormatArg::Json => OutputFormat::Json,
        }
    }
}

fn main() {
    env_logger::init();

    let cli = Cli::parse();

    if cli.init {
        match Session::open(&cli.db, true) {
            Ok(_) => {
                eprintln!("Initialized database at {}", cli.db.display());
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error initializing database: {}", e);
                process::exit(3);
            }
        }
    }

    let mut session = match Session::open(&cli.db, cli.quiet) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error opening database '{}': {}", cli.db.display(), e);
            process::exit(3);
        }
    };

    // Set output format
    session.set_format(cli.format.into());

    if let Some(sql) = &cli.eval {
        match run_eval(&mut session, sql, cli.continue_on_error) {
            Ok(()) => process::exit(0),
            Err(code) => process::exit(code),
        }
    } else if let Some(path) = &cli.file {
        match run_file(&mut session, path, cli.continue_on_error) {
            Ok(()) => process::exit(0),
            Err(code) => process::exit(code),
        }
    } else {
        // REPL mode: triggered when stdin is a TTY or --repl is passed
        let is_tty = std::io::stdin().is_terminal();
        if is_tty || cli.repl {
            match run_repl(&mut session) {
                Ok(()) => process::exit(0),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        } else {
            // Read SQL from stdin in batch mode
            let mut sql = String::new();
            match std::io::read_to_string(std::io::stdin()) {
                Ok(s) => sql = s,
                Err(e) => {
                    eprintln!("Error reading stdin: {}", e);
                    process::exit(3);
                }
            }
            match run_eval(&mut session, &sql, cli.continue_on_error) {
                Ok(()) => process::exit(0),
                Err(code) => process::exit(code),
            }
        }
    }
}

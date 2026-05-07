fn main() {
    env_logger::init();
    match minidb::cli::run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e}");
            let code = match e {
                minidb::MiniDbError::Sql(_)
                | minidb::MiniDbError::Parse { .. }
                | minidb::MiniDbError::Type(_) => 1,
                minidb::MiniDbError::Cli(_) => 2,
                minidb::MiniDbError::Io(_) | minidb::MiniDbError::Storage(_) => 3,
                minidb::MiniDbError::Txn(_) => 1,
            };
            std::process::exit(code);
        }
    }
}

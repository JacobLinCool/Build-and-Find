use crate::cli::format::{format_result, OutputFormat};
use crate::error::MiniDbError;
use crate::exec::Database;
use crate::parser::ast::MetaCmd;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io::{self, IsTerminal, Write};

pub struct Repl {
    format: OutputFormat,
    quiet: bool,
}

impl Repl {
    pub fn new(format: OutputFormat, quiet: bool) -> Self {
        Repl { format, quiet }
    }

    pub fn run(&mut self, db: &mut Database) -> i32 {
        if io::stdin().is_terminal() {
            self.run_interactive(db)
        } else {
            self.run_piped(db)
        }
    }

    fn run_interactive(&mut self, db: &mut Database) -> i32 {
        let mut rl = DefaultEditor::new().unwrap_or_else(|_| DefaultEditor::new().unwrap());
        let prompt = "minidb> ";
        let mut buf = String::new();

        println!("MiniDB -- type SQL statements terminated by ';'");
        println!("Type .exit or .quit to quit.");

        loop {
            let p = if buf.is_empty() { prompt } else { "     -> " };
            let line = rl.readline(p);
            match line {
                Ok(input) => {
                    rl.add_history_entry(&input).ok();
                    buf.push_str(&input);
                    buf.push('\n');

                    // Check for meta-commands on their own line
                    let is_meta = buf.trim().starts_with('.');
                    let ends_semi = buf.trim().ends_with(';');
                    let is_empty = buf.trim().is_empty();

                    if is_meta {
                        let cmd = buf.trim().to_string();
                        buf.clear();
                        let rc = self.handle_input(db, &cmd);
                        if cmd.contains("exit") || cmd.contains("quit") {
                            return 0;
                        }
                        continue;
                    }

                    // Execute when we have a complete statement (ends with ;)
                    if ends_semi || is_empty {
                        if !is_empty {
                            let stmt = buf.trim().to_string();
                            self.handle_input(db, &stmt);
                        }
                        buf.clear();
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    buf.clear();
                    println!("^C");
                }
                Err(ReadlineError::Eof) => {
                    if !buf.trim().is_empty() {
                        self.handle_input(db, buf.trim());
                    }
                    println!();
                    return 0;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return 1;
                }
            }
        }
    }

    fn run_piped(&mut self, db: &mut Database) -> i32 {
        use std::io::BufRead;
        let stdin = io::stdin();
        let mut buf = String::new();
        let mut last_rc = 0i32;

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("read error: {}", e);
                    return 3;
                }
            };
            let trimmed = line.trim();
            if trimmed.starts_with('.') {
                last_rc = self.handle_input(db, trimmed).unwrap_or(0);
                if trimmed == ".exit" || trimmed == ".quit" {
                    return 0;
                }
                continue;
            }
            buf.push_str(&line);
            buf.push('\n');

            // Execute on semicolon
            if trimmed.ends_with(';') {
                last_rc = self.handle_input(db, &buf).unwrap_or(0);
                buf.clear();
            }
        }

        // Execute any remaining
        if !buf.trim().is_empty() {
            last_rc = self.handle_input(db, &buf).unwrap_or(0);
        }

        last_rc
    }

    fn handle_input(&mut self, db: &mut Database, sql: &str) -> Option<i32> {
        match self.execute_sql(db, sql) {
            Ok(exit) => exit,
            Err(e) => {
                eprintln!("Error: {}", e);
                Some(1)
            }
        }
    }

    fn execute_sql(&mut self, db: &mut Database, sql: &str) -> crate::error::Result<Option<i32>> {
        use crate::parser::parse;
        use crate::plan::Planner;

        let stmts = parse(sql).map_err(|e| e)?;

        for stmt in stmts {
            // Check for .format meta-command
            if let crate::parser::ast::Statement::MetaCommand(MetaCmd::Exit) = &stmt {
                return Ok(Some(0));
            }
            if let crate::parser::ast::Statement::MetaCommand(MetaCmd::Format(f)) = &stmt {
                if let Some(fmt) = OutputFormat::from_str(f) {
                    self.format = fmt;
                    if !self.quiet {
                        eprintln!("ok format changed to {}", f);
                    }
                    continue;
                }
            }

            let planner = Planner::new(&db.catalog);
            let plan = planner.plan_statement(stmt)?;
            let result = db.execute_plan(plan)?;

            if let Some(msg) = &result.message {
                if msg == "exit" {
                    return Ok(Some(0));
                }
                if !self.quiet {
                    eprintln!("ok {}", msg);
                }
                continue;
            }

            if let Some(n) = result.affected_rows {
                // Print DML summary to stderr
                if !self.quiet {
                    let word = if n == 1 { "row" } else { "rows" };
                    eprintln!("ok {} {} affected", n, word);
                }
                continue;
            }

            // SELECT result -- print to stdout
            if !result.rows.is_empty() || !result.schema.columns.is_empty() {
                print!("{}", format_result(&result, &self.format));
                if !self.quiet {
                    let n = result.rows.len();
                    let word = if n == 1 { "row" } else { "rows" };
                    eprintln!("ok {} {}", n, word);
                }
            }
        }
        Ok(None)
    }
}

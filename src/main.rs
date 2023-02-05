extern crate core;

use crate::engine::model_checker;
use crate::parser::{Parser, ParserErrorKind};
use crate::reporter::summary;
use std::env;
use std::fs::read_to_string;

mod engine;
mod interpreter;
mod parser;
mod reporter;
mod scanner;
mod sql_interpreter;

fn main() {
    let args: Vec<String> = env::args().collect();
    let default_file = "./model.mets".to_string();
    let file = args.get(1).unwrap_or(&default_file);
    let source = read_to_string(file).unwrap_or_else(|_| format!("Could not open {file}"));
    let parser = Parser::new(source);

    let res = parser.compile();

    match res {
        Ok(mets) => match model_checker(&mets) {
            Ok(report) => println!("{}", summary(&mets, &report)),
            Err(message) => println!("{message}",),
        },
        Err(message) => match message.kind {
            ParserErrorKind::ParseInt(err) => println!(
                "Error at {file}:{}:{}: Could not parse integer from lexeme {:?}: {err:?}",
                message.current.position.start_line,
                message.current.position.start_col,
                message.current.lexeme
            ),
            ParserErrorKind::Scanner(err) => println!(
                "Error at {file}:{}:{}: Could not parse token {:?}: {err:?}",
                message.current.position.start_line,
                message.current.position.start_col,
                message.current.lexeme
            ),
            ParserErrorKind::Unexpected(err) => println!(
                "Error at {file}:{}:{}: Unexpected token {:?}: {err}",
                message.current.position.start_line,
                message.current.position.start_col,
                message.current.lexeme
            ),
        },
    }
}

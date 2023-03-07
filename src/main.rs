use crate::engine::{model_checker, CheckerError};
use crate::interpreter::InterpreterError;
use crate::parser::{Parser, ParserErrorKind};
use crate::reporter::summary;
use std::env;
use std::fs::read_to_string;

mod engine;
mod format;
mod interpreter;
mod parser;
mod reporter;
mod scanner;
mod sql_interpreter;
mod state;

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
            Err(err) => match err {
                CheckerError::InterpreterError(err) => match err {
                    InterpreterError::Unexpected(expr) => println!("Unexpected: {expr}"),
                    InterpreterError::TypeError(x, y, z) => {
                        println!("Expected '{x}' to be a {z}, was {y} ")
                    }
                    InterpreterError::SqlEngineError(w) => println!("Sql Engine Error: {w:?}"),
                },
            },
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
            ParserErrorKind::AggregateError(item) => println!(
                "Error at {file}:{}:{}: Column {item} must appear in group by",
                message.current.position.start_line, message.current.position.start_col
            ),
        },
    }
}

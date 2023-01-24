#![allow(dead_code)]

extern crate core;

use crate::engine::model_checker;
use crate::parser::Parser;
use crate::reporter::summary;
use std::env;
use std::fs::read_to_string;

mod engine;
mod interpreter;
mod parser;
mod reporter;
mod scanner;
mod sql_engine;

fn main() {
    let args: Vec<String> = env::args().collect();
    let source = read_to_string(args.get(1).unwrap_or(&"./model.mets".to_string()))
        .expect("expected a model.mets file");
    let parser = Parser::new(source);

    let res = parser.compile();

    match res {
        Ok(mets) => match model_checker(&mets) {
            Ok(report) => println!("{}", summary(&mets, &report)),
            Err(message) => println!("{}", message),
        },
        Err(message) => println!("{}", message),
    }
}

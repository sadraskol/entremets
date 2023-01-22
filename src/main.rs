#![allow(dead_code)]

use crate::parser::Parser;
use std::env;
use std::fs::read_to_string;

mod engine;
mod parser;
mod scanner;

fn main() {
    let args: Vec<String> = env::args().collect();
    let source = read_to_string(args.get(1).unwrap_or(&"./model.mets".to_string()))
        .expect("expected a model.mets file");
    let parser = Parser::new(source);

    let res = parser.compile();

    match res {
        Ok(_mets) => {} //println!("{:?}", _mets)
        Err(message) => println!("{}", message),
    }
}

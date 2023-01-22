#![allow(dead_code)]

use crate::scanner::{Scanner, TokenKind};
use std::env;
use std::fs::read_to_string;

mod engine;
mod parser;
mod scanner;

fn main() {
    let args: Vec<String> = env::args().collect();
    let source = read_to_string(args.get(1).unwrap_or(&"./model.mets".to_string()))
        .expect("expected a model.mets file");
    let mut scanner = Scanner::new(source);
    let mut current = scanner.scan_token();
    while let Ok(token) = &current {
        if token.kind == TokenKind::Eof {
            break;
        } else {
            println!("{:?}", token);
        }
        current = scanner.scan_token();
    }
    if let Err(err) = &current {
        println!("{:?}", err);
    }
}

fn main() {
    println!("Hello, world!");
}

struct Process {
    name: String,
    statements: Vec<Statement>,
}

enum Statement {
    Transaction(Transaction),
    Block(Vec<Statement>),
    Assignment(Variable, Sigma),
    // Assignment(Variable, Expression)
    If(Proposition, Box<Statement>),
    Insert(Relation, Tuple),
}

enum IsolationLevel {
    ReadCommitted,
}

struct Transaction {
    isolation: IsolationLevel,
    statement: Box<Statement>,
}

struct Variable {
    name: String,
}

struct Relation {
    name: String,
}

struct Tuple {
    value: Vec<(Variable, Number)>, // Vec<(Variable, Expression?)>
}

struct Number {
    value: i16,
}

enum Proposition {
    Count(Box<Proposition>),
    And(Box<Proposition>, Box<Proposition>),
    Equal(Box<Proposition>, Box<Proposition>),
    LesserEqual(Box<Proposition>, Box<Proposition>),
    Literal(Number),
    // Literal(Literal)
    Var(Variable),
}

struct Sigma {
    formula: Proposition,
    relation: Relation,
}

#[cfg(test)]
mod test {
    use crate::{
        IsolationLevel, Number, Operation, Process, Proposition, Relation, Sigma, Statement,
        Transaction, Tuple, Variable,
    };

    #[test]
    fn run_process_with_correct_isolation_level() {
        let mut processes = vec![];
        for i in 1..=3 {
            processes.push(Process {
                name: i.to_string(),
                statements: vec![Statement::Transaction(Transaction {
                    isolation: IsolationLevel::ReadCommitted,
                    statement: Box::new(Statement::Block(vec![
                        Statement::Assignment(
                            Variable {
                                name: "under_18".to_string(),
                            },
                            Sigma {
                                formula: Proposition::LesserEqual(
                                    Box::new(Proposition::Var(Variable {
                                        name: "age".to_string(),
                                    })),
                                    Box::new(Proposition::Literal(Number { value: 18 })),
                                ),
                                relation: Relation {
                                    name: "users".to_string(),
                                },
                            },
                        ),
                        Statement::If(
                            Proposition::Count(Box::new(Proposition::Var(Variable {
                                name: "under_18".to_string(),
                            }))),
                            Box::new(Statement::Block(vec![Statement::Insert(
                                Relation {
                                    name: "users".to_string(),
                                },
                                Tuple { value: vec![(Variable { name: "age".to_string() }, Number { value: 12 })] },
                            )])),
                        ),
                    ])),
                })],
            })
        }
        // properties
        // runner
    }
}

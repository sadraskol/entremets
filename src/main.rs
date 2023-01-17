extern crate core;

use std::collections::{HashMap, HashSet, VecDeque};

fn main() {
    println!("Hello, world!");
}

#[derive(PartialEq, Debug)]
struct Client {
    name: String,
    statements: Vec<Statement>,
}

#[derive(PartialEq, Debug)]
enum Statement {
    Begin(IsolationLevel),
    Commit,
    Assignment(Variable, Sigma),
    // Assignment(Variable, Expression)
    If(Proposition, usize),
    // offset
    Insert(Relation, Row),
}

#[derive(PartialEq, Debug)]
enum IsolationLevel {
    ReadCommitted,
}

#[derive(PartialEq, Debug)]
struct Transaction {
    isolation: IsolationLevel,
    statement: Box<Statement>,
}

#[derive(PartialEq, Debug, Clone)]
struct Variable {
    name: String,
}

#[derive(PartialEq, Debug, Clone)]
struct Relation {
    name: String,
}

#[derive(PartialEq, Debug, Clone)]
struct Property(Proposition);

#[derive(PartialEq, Debug, Clone)]
enum Proposition {
    Count(Box<Proposition>),
    Sigma(Box<Sigma>),
    LesserEqual(Box<Proposition>, Box<Proposition>),
    Literal(Value),
    Always(Box<Proposition>),
    // Literal(Literal)
    Var(Variable),
}

#[derive(PartialEq, Debug, Clone)]
struct Sigma {
    formula: Proposition,
    relation: Relation,
}

#[derive(PartialEq, Debug, Clone)]
struct Violation {
    property: Property,
    log: Vec<Trace>,
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct Row {
    keys: Vec<String>,
    values: Vec<Value>,
}

impl Row {
    pub fn get(&self, key: &String) -> Option<Value> {
        for (i, k) in self.keys.iter().enumerate() {
            if k == key {
                return Some(self.values[i].clone());
            }
        }
        None
    }
}

impl Row {
    fn new() -> Self {
        Row {
            keys: vec![],
            values: vec![],
        }
    }
}

impl<const N: usize> From<[(String, Value); N]> for Row {
    fn from(arr: [(String, Value); N]) -> Self {
        let mut res = Row::new();
        for entry in arr {
            res.keys.push(entry.0);
            res.values.push(entry.1);
        }
        res
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
enum Value {
    Bool(bool),
    Integer(i16),
    Row(Row),
    Set(Vec<Value>),
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct HashableState {
    pc: Vec<usize>,
    global: Vec<(String, Value)>,
    locals: Vec<Vec<(String, Value)>>,
}

#[derive(PartialEq, Debug, Clone)]
struct Trace {
    pc: Vec<usize>,
    global: HashMap<String, Value>,
    locals: Vec<HashMap<String, Value>>,
}

#[derive(PartialEq, Debug, Clone)]
struct State {
    pc: Vec<usize>,
    global: HashMap<String, Value>,
    locals: Vec<HashMap<String, Value>>,
    log: Vec<Trace>,
}

impl State {
    fn trace(&self) -> Trace {
        Trace {
            pc: self.pc.clone(),
            global: self.global.clone(),
            locals: self.locals.clone(),
        }
    }

    fn hashable(&self) -> HashableState {
        HashableState {
            pc: self.pc.clone(),
            global: self
                .global
                .iter()
                .map(|(l, r)| (l.clone(), r.clone()))
                .collect(),
            locals: self
                .locals
                .iter()
                .map(|l| l.iter().map(|(l, r)| (l.clone(), r.clone())).collect())
                .collect(),
        }
    }
}

fn eval(state: &State, select: &Sigma) -> Value {
    let binding = Value::Set(vec![]);
    let from = state.global.get(&select.relation.name).unwrap_or(&binding);
    match &select.formula {
        Proposition::LesserEqual(left, right) => {
            let mut res = vec![];
            let rows = if let Value::Set(rows) = from {
                rows
            } else {
                todo!()
            };
            for row in rows {
                let rw = if let Value::Row(rw) = row {
                    rw
                } else {
                    todo!()
                };
                let l = if let Value::Integer(l) = eval_prop_row(rw, &left) {
                    l
                } else {
                    todo!()
                };
                let r = if let Value::Integer(r) = eval_prop_row(rw, &right) {
                    r
                } else {
                    todo!()
                };
                if l <= r {
                    res.push(row.clone());
                }
            }
            Value::Set(res)
        }
        _ => panic!("{:?} not yet implemented", select.formula),
    }
}

fn eval_prop_local(state: &State, local: &HashMap<String, Value>, prop: &Proposition) -> Value {
    match prop {
        Proposition::LesserEqual(left, right) => {
            let l = if let Value::Integer(l) = eval_prop_local(state, local, &left) {
                l
            } else {
                todo!()
            };
            let r = if let Value::Integer(r) = eval_prop_local(state, local, &right) {
                r
            } else {
                todo!()
            };
            Value::Bool(l <= r)
        }
        Proposition::Count(set) => {
            let s = if let Value::Set(s) = eval_prop_local(state, local, &set) {
                s
            } else {
                todo!()
            };
            Value::Integer(i16::try_from(s.len()).expect("set is too large for count"))
        }
        Proposition::Var(var) => local.get(&var.name).expect("oupsy").clone(),
        Proposition::Literal(value) => value.clone(),
        Proposition::Sigma(sigma) => eval(state, sigma),
        _ => panic!("{:?} is not implemented for evaluation", prop),
    }
}

fn eval_prop_row(row: &Row, prop: &Proposition) -> Value {
    match prop {
        Proposition::Literal(value) => value.clone(),
        Proposition::Var(v) => row
            .get(&v.name)
            .expect(&format!("couldn't find column '{}' in table...", v.name))
            .clone(),
        _ => panic!("{:?} is not implemented for row evaluation", prop),
    }
}

fn apply_statement(state: &State, idx: usize, client: &Client) -> State {
    let mut new_state = state.clone();
    match &client.statements[state.pc[idx]] {
        Statement::Begin(_) => {
            new_state.pc[idx] += 1;
            new_state
        }
        Statement::Commit => {
            new_state.pc[idx] += 1;
            new_state
        }
        Statement::Assignment(v, expression) => {
            new_state.pc[idx] += 1;
            new_state.locals[idx].insert(v.name.clone(), eval(state, &expression));
            new_state
        }
        Statement::If(condition, offset) => {
            let proposition_res = eval_prop_local(&state, &state.locals[idx], condition);
            if proposition_res == Value::Bool(true) {
                new_state.pc[idx] += 1;
                new_state
            } else if proposition_res == Value::Bool(false) {
                new_state.pc[idx] += 1 + offset;
                new_state
            } else {
                panic!("condition is not a boolean");
            }
        }
        Statement::Insert(relation, row) => {
            new_state.pc[idx] += 1;
            let table = if let Value::Set(table) = new_state
                .global
                .entry(relation.name.clone())
                .or_insert(Value::Set(vec![]))
            {
                table
            } else {
                todo!()
            };
            table.push(Value::Row(row.clone()));
            new_state.global.iter_mut();
            new_state
        }
    }
}

fn check_property(state: &State, property: &Property) -> Value {
    let prop = if let Property(Proposition::Always(prop)) = property {
        prop
    } else {
        todo!()
    };
    eval_prop_local(state, &state.global, prop)
}

struct Report {
    violation: Option<Violation>,
}

fn model_checker(clients: &Vec<Client>, properties: &Vec<Property>) -> Report {
    let init_state = State {
        pc: clients.iter().map(|_| 0).collect(),
        global: HashMap::new(),
        locals: clients.iter().map(|_| HashMap::new()).collect(),
        log: vec![],
    };

    let mut deq = VecDeque::from([init_state]);
    let mut visited = HashSet::new();
    let mut state_checked: u128 = 0;

    while let Some(state) = deq.pop_front() {
        // println!("checking for state: {:?}", state);
        if visited.contains(&state.hashable()) {
            continue;
        }
        visited.insert(state.hashable());

        for property in properties {
            if check_property(&state, property) != Value::Bool(true) {
                let mut log = state.log.clone();
                log.push(state.trace());
                return Report {
                    violation: Some
                        (Violation {
                            property: property.clone(),
                            log,
                        })
                };
            }
        }

        state_checked += 1;
        for (idx, client) in clients.iter().enumerate() {
            if state.pc[idx] < client.statements.len() {
                let mut new_state = apply_statement(&state, idx, client);
                new_state.log.push(state.trace());
                deq.push_back(new_state);
            }
        }
    }
    println!("state_checked {}", state_checked);

    Report {
        violation: None
    }
}

fn summary(report: &Report) -> String {
    if let Some(violation) = &report.violation {
        let mut x = format!("Following property was wrong: {:?}\n", violation.property);
        x.push_str("The following counter example was found:\n");
        for trace in &violation.log {
            x.push_str(&format!("{:?}\n", trace));
        }
        x
    } else {
        "No counter example found".to_string()
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    #[test]
    fn run_process_with_correct_isolation_level() {
        let mut clients = vec![];
        for i in 1..=3 {
            clients.push(Client {
                name: i.to_string(),
                statements: vec![
                    Statement::Begin(IsolationLevel::ReadCommitted),
                    Statement::Assignment(
                        Variable {
                            name: "under_18".to_string(),
                        },
                        Sigma {
                            formula: Proposition::LesserEqual(
                                Box::new(Proposition::Var(Variable {
                                    name: "age".to_string(),
                                })),
                                Box::new(Proposition::Literal(Value::Integer(18))),
                            ),
                            relation: Relation {
                                name: "users".to_string(),
                            },
                        },
                    ),
                    Statement::If(
                        Proposition::LesserEqual(
                            Box::new(Proposition::Count(Box::new(Proposition::Var(Variable {
                                name: "under_18".to_string(),
                            })))),
                            Box::new(Proposition::Literal(Value::Integer(1))),
                        ),
                        1,
                    ),
                    Statement::Insert(
                        Relation {
                            name: "users".to_string(),
                        },
                        Row::from([("age".to_string(), Value::Integer(12))]),
                    ),
                    Statement::Commit,
                ],
            });
        }

        let properties = vec![Property(Proposition::Always(Box::new(
            Proposition::LesserEqual(
                Box::new(Proposition::Count(Box::new(Proposition::Sigma(Box::new(
                    Sigma {
                        formula: Proposition::LesserEqual(
                            Box::new(Proposition::Var(Variable {
                                name: "age".to_string(),
                            })),
                            Box::new(Proposition::Literal(Value::Integer(18))),
                        ),
                        relation: Relation {
                            name: "users".to_string(),
                        },
                    },
                ))))),
                Box::new(Proposition::Literal(Value::Integer(2))),
            ),
        )))];

        let report = model_checker(&clients, &properties);

        println!("{}", summary(&report));
        assert_eq!("".to_string(), summary(&report));
    }
}

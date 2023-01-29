use crate::interpreter::{Interpreter, InterpreterError};
use crate::parser::{Mets, Statement};
use crate::sql_engine::{HashableRow, SqlDatabase, SqlEngineError, TransactionId};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Integer(i16),
    Set(Vec<Value>),
    Tuple(Vec<Value>),
}

impl Value {
    pub fn to_string(&self) -> String {
        match self {
            Value::Nil => "nil".to_string(),
            Value::Bool(x) => {
                if *x {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Value::Integer(i) => i.to_string(),
            Value::Set(set) => {
                let mut res = "{".to_string();
                let mut peekable = set.iter().peekable();
                while let Some(i) = peekable.next() {
                    res += &i.to_string();
                    if peekable.peek().is_some() {
                        res = res.to_owned() + ", ";
                    }
                }

                res += "}";

                res
            }
            Value::Tuple(tuple) => {
                let mut res = "(".to_string();
                let mut peekable = tuple.iter().peekable();
                while let Some(i) = peekable.next() {
                    res += &i.to_string();
                    if peekable.peek().is_some() {
                        res = res.to_owned() + ", ";
                    }
                }

                res += ")";

                res
            }
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct HashableState {
    pc: Vec<usize>,
    global: Vec<(String, Vec<HashableRow>)>,
    locals: Vec<(String, Value)>,
    eventually: Vec<(usize, bool)>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Trace {
    pub pc: Vec<usize>,
    pub sql: SqlDatabase,
    pub locals: HashMap<String, Value>,
}

#[derive(PartialEq, Debug, Clone)]
pub enum ProcessState {
    Running,
    Waiting,
    Finished,
}

#[derive(PartialEq, Debug, Clone)]
pub struct State {
    pc: Vec<usize>,
    pub state: Vec<ProcessState>,
    pub txs: Vec<Option<TransactionId>>,
    pub sql: SqlDatabase,
    pub locals: HashMap<String, Value>,
    log: Vec<Trace>,
    eventually: HashMap<usize, bool>,
}

impl State {
    fn trace(&self) -> Trace {
        Trace {
            pc: self.pc.clone(),
            sql: self.sql.clone(),
            locals: self.locals.clone(),
        }
    }

    fn hashable(&self) -> HashableState {
        HashableState {
            pc: self.pc.clone(),
            global: self.sql.hashable(),
            locals: self
                .locals
                .iter()
                .map(|(l, r)| (l.clone(), r.clone()))
                .collect(),
            eventually: self.eventually.iter().map(|(l, r)| (*l, *r)).collect(),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Violation {
    pub property: Statement,
    pub log: Vec<Trace>,
}

pub struct Report {
    pub states_explored: usize,
    pub violation: Option<Violation>,
}

#[derive(Debug)]
pub enum CheckerError {
    InterpreterError(InterpreterError),
}

impl From<InterpreterError> for CheckerError {
    fn from(value: InterpreterError) -> Self {
        CheckerError::InterpreterError(value)
    }
}

type Res<T> = Result<T, CheckerError>;
type Unit = Res<()>;

pub fn model_checker(mets: &Mets) -> Result<Report, String> {
    match private_model_checker(mets) {
        Ok(res) => Ok(res),
        Err(err) => Err(format!("{:?}", err)),
    }
}

pub enum PropertyCheck {
    Always(bool),
    Eventually(bool),
}

fn private_model_checker(mets: &Mets) -> Res<Report> {
    let init_state = init_state(mets)?;

    let mut deq = VecDeque::from([init_state]);
    let mut visited = HashSet::new();

    let mut states_explored = 0;

    while let Some(mut state) = deq.pop_front() {
        if visited.contains(&state.hashable()) {
            continue;
        }
        visited.insert(state.hashable());

        let mut interpreter = Interpreter::new(&state);

        for (id, property) in mets.properties.iter().enumerate() {
            let res = interpreter.check_property(property)?;
            match res {
                PropertyCheck::Always(false) => {
                    let mut log = state.log.clone();
                    log.push(state.trace());
                    return Ok(Report {
                        states_explored,
                        violation: Some(Violation {
                            property: property.clone(),
                            log,
                        }),
                    });
                }
                PropertyCheck::Eventually(res) => {
                    let existing = state.eventually.entry(id).or_insert(false);
                    if !*existing && res {
                        *existing = res;
                    }
                }
                _ => {}
            }
        }

        states_explored += 1;

        let mut is_final = true;
        for (idx, code) in mets.processes.iter().enumerate() {
            if state.state[idx] == ProcessState::Running {
                interpreter.idx = idx;
                if let Err(InterpreterError::SqlEngineError(SqlEngineError::RowLockedError)) =
                    interpreter.statement(&code[state.pc[idx]])
                {
                    interpreter.reset();
                    continue;
                }
                let mut new_state = interpreter.reset();
                new_state.pc[idx] += 1;

                if new_state.pc[idx] == code.len() {
                    new_state.state[idx] = ProcessState::Finished
                }
                if new_state
                    .state
                    .iter()
                    .all(|w| w == &ProcessState::Waiting || w == &ProcessState::Finished)
                {
                    for w in new_state.state.iter_mut() {
                        if w == &ProcessState::Waiting {
                            *w = ProcessState::Running;
                        }
                    }
                }

                new_state.log.push(state.trace());
                deq.push_back(new_state);
                is_final = false;
            }
        }

        if is_final {
            if let Some((id, _)) = state.eventually.iter().find(|(_, b)| !**b) {
                let mut log = state.log.clone();
                log.push(state.trace());
                return Ok(Report {
                    states_explored,
                    violation: Some(Violation {
                        property: mets.properties[*id].clone(),
                        log,
                    }),
                });
            }
        }
    }

    Ok(Report {
        states_explored,
        violation: None,
    })
}

fn init_state(mets: &Mets) -> Res<State> {
    let init_state = State {
        pc: mets.processes.iter().map(|_| 0).collect(),
        state: mets
            .processes
            .iter()
            .map(|_| ProcessState::Running)
            .collect(),
        txs: mets.processes.iter().map(|_| None).collect(),
        sql: SqlDatabase::new(),
        locals: HashMap::new(),
        log: vec![],
        eventually: HashMap::new(),
    };
    let mut interpreter = Interpreter::new(&init_state);
    for statement in &mets.init {
        interpreter.statement(statement)?;
    }
    Ok(interpreter.reset())
}

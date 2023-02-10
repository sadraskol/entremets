use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Formatter;

use crate::format::intersperse;
use crate::interpreter::{Interpreter, InterpreterError};
use crate::parser::{Mets, Statement};
use crate::sql_interpreter::{SqlDatabase, TransactionId};
use crate::state::{HashableState, ProcessState, RcState, State, TransactionInfo};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub enum TransactionState {
    NotExisting,
    Running,
    Aborted,
    Committed,
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct Transaction(pub TransactionState);

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub enum Value {
    Nil,
    Tx(Transaction),
    Bool(bool),
    Integer(i16),
    Set(Vec<Value>),
    Tuple(Vec<Value>),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Nil => f.write_str("nil"),
            Value::Bool(x) => {
                if *x {
                    f.write_str("true")
                } else {
                    f.write_str("false")
                }
            }
            Value::Integer(i) => std::fmt::Display::fmt(&i, f),
            Value::Set(set) => {
                f.write_str("{")?;
                intersperse(f, set, ",")?;
                f.write_str("}")
            }
            Value::Tuple(tuple) => {
                f.write_str("(")?;
                intersperse(f, tuple, ",")?;
                f.write_str(")")
            }
            Value::Tx(tx) => match tx.0 {
                TransactionState::NotExisting => f.write_str("non started transaction"),
                TransactionState::Running => f.write_str("running transaction"),
                TransactionState::Aborted => f.write_str("aborted transaction"),
                TransactionState::Committed => f.write_str("committed transaction"),
            },
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum Violation {
    PropertyViolation {
        property: Statement,
        state: RcState,
    },
    Deadlock {
        cycle: HashSet<usize>,
        state: RcState,
    },
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

pub fn model_checker(mets: &Mets) -> Result<Report, String> {
    match private_model_checker(mets) {
        Ok(res) => Ok(res),
        Err(err) => Err(format!("{err:?}")),
    }
}

pub enum PropertyCheck {
    Always(bool),
    Eventually(bool),
}

fn private_model_checker(mets: &Mets) -> Res<Report> {
    let init_state = init_state(mets)?;

    let mut deq = VecDeque::from([RcState::new(init_state)]);
    let mut visited: HashMap<HashableState, RcState> = HashMap::new();

    let mut states_explored = 0;

    while let Some(state) = deq.pop_front() {
        let hashed_state = state.borrow().hash();
        if let Some(existing_state) = visited.get_mut(&hashed_state) {
            let mut st = existing_state.borrow_mut();
            st.ancestors.extend_from_slice(&state.borrow().ancestors);
            continue;
        }
        visited.insert(hashed_state, state.clone());

        let mut interpreter = Interpreter::new(state.clone());

        for (id, property) in mets.properties.iter().enumerate() {
            let res = interpreter.check_property(property)?;
            match res {
                PropertyCheck::Always(false) => {
                    return Ok(Report {
                        states_explored,
                        violation: Some(Violation::PropertyViolation {
                            property: property.clone(),
                            state,
                        }),
                    });
                }
                PropertyCheck::Eventually(res) => {
                    let mut state = state.borrow_mut();
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
            if state.borrow().processes[idx] == ProcessState::Running {
                interpreter.idx = idx;
                let offset = interpreter.statement(&code[state.borrow().pc[idx]])?;
                let mut new_state = interpreter.next_state();

                new_state.pc[idx] += offset;
                new_state.ancestors = vec![state.clone()];
                if new_state.pc[idx] == code.len() {
                    new_state.processes[idx] = ProcessState::Finished
                }

                if let Some(deadlock_cycle) = new_state.find_deadlocks() {
                    return Ok(Report {
                        states_explored,
                        violation: Some(Violation::Deadlock {
                            cycle: deadlock_cycle,
                            state: RcState::new(new_state),
                        }),
                    });
                }

                new_state.unlock_locks();
                new_state.unlock_latches();

                deq.push_back(RcState::new(new_state));
                is_final = false;
            }
        }

        if is_final {
            if let Some((id, _)) = state.borrow().eventually.iter().find(|(_, b)| !**b) {
                return Ok(Report {
                    states_explored,
                    violation: Some(Violation::PropertyViolation {
                        property: mets.properties[*id].clone(),
                        state: state.clone(),
                    }),
                });
            };
        };
    }

    Ok(Report {
        states_explored,
        violation: None,
    })
}

fn init_state(mets: &Mets) -> Res<State> {
    let init_state = State {
        pc: mets.processes.iter().map(|_| 0).collect(),
        processes: mets
            .processes
            .iter()
            .map(|_| ProcessState::Running)
            .collect(),
        txs: mets
            .processes
            .iter()
            .map(|_| TransactionInfo {
                id: TransactionId(usize::MAX),
                name: None,
                state: TransactionState::NotExisting,
            })
            .collect(),
        sql: SqlDatabase::new(),
        locals: HashMap::new(),
        ancestors: vec![],
        eventually: HashMap::new(),
    };
    let mut interpreter = Interpreter::new(RcState::new(init_state));
    for statement in &mets.init {
        interpreter.statement(statement)?;
    }
    Ok(interpreter.next_state())
}

use std::cell::RefCell;
use crate::format::intersperse;
use crate::interpreter::{Interpreter, InterpreterError};
use crate::parser::{Mets, Statement};
use crate::sql_interpreter::{HashableRow, RowId, SqlDatabase, TransactionId};
use std::collections::{HashMap, VecDeque};
use std::fmt::Formatter;
use std::rc::Rc;

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
    Latching,
    Locked(RowId),
    Finished,
}

#[derive(PartialEq, Debug, Clone)]
pub struct TransactionInfo {
    pub id: TransactionId,
    pub name: Option<String>,
    pub state: TransactionState,
}

#[derive(PartialEq, Debug, Clone)]
pub struct State {
    pub pc: Vec<usize>,
    pub state: Vec<ProcessState>,
    pub txs: Vec<TransactionInfo>,
    pub sql: SqlDatabase,
    pub locals: HashMap<String, Value>,
    pub ancestors: Vec<RcState>,
    eventually: HashMap<usize, bool>,
}

impl State {
    fn hash(&self) -> HashableState {
        HashableState {
            pc: self.pc.clone(),
            global: self.sql.hash(),
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
    pub state: RcState,
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

pub type RcState = Rc<RefCell<State>>;

fn rc_state(state: State) -> RcState {
    Rc::new(RefCell::new(state))
}

fn private_model_checker(mets: &Mets) -> Res<Report> {
    let init_state = init_state(mets)?;

    let mut deq = VecDeque::from([rc_state(init_state)]);
    let mut visited: HashMap<HashableState, RcState> = HashMap::new();

    let mut states_explored = 0;

    while let Some(state) = deq.pop_front() {
        let hashed_state = RefCell::borrow(&state).hash();
        if let Some(existing_state) = visited.get_mut(&hashed_state) {
            let mut st = RefCell::borrow_mut(existing_state);
            st.ancestors.extend_from_slice(&RefCell::borrow(&state).ancestors);
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
                        violation: Some(Violation {
                            property: property.clone(),
                            state,
                        }),
                    });
                }
                PropertyCheck::Eventually(res) => {
                    let mut state = RefCell::borrow_mut(&state);
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
            if RefCell::borrow(&state).state[idx] == ProcessState::Running {
                interpreter.idx = idx;
                let offset = interpreter.statement(&code[RefCell::borrow(&state).pc[idx]])?;
                let mut new_state = interpreter.next_state();
                new_state.pc[idx] += offset;

                if new_state.pc[idx] == code.len() {
                    new_state.state[idx] = ProcessState::Finished
                }
                if new_state
                    .state
                    .iter()
                    .all(|w| w == &ProcessState::Latching || w == &ProcessState::Finished)
                {
                    for w in new_state.state.iter_mut() {
                        if w == &ProcessState::Latching {
                            *w = ProcessState::Running;
                        }
                    }
                }

                new_state.ancestors = vec![state.clone()];
                deq.push_back(rc_state(new_state));
                is_final = false;
            }
        }

        if is_final {
            if let Some((id, _)) = RefCell::borrow(&state).eventually.iter().find(|(_, b)| !**b) {
                return Ok(Report {
                    states_explored,
                    violation: Some(Violation {
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
        state: mets
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
    let mut interpreter = Interpreter::new(rc_state(init_state));
    for statement in &mets.init {
        interpreter.statement(statement)?;
    }
    Ok(interpreter.next_state())
}

use crate::engine::{TransactionState, Value};
use crate::sql_interpreter::{HashableRow, RowId, SqlDatabase, TransactionId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HashableState {
    pc: Vec<usize>,
    state: Vec<ProcessState>,
    global: Vec<(String, Vec<HashableRow>)>,
    locals: Vec<(String, Value)>,
    eventually: Vec<(usize, bool)>,
}

#[derive(PartialEq, Debug, Clone, Hash, Eq)]
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
    pub processes: Vec<ProcessState>,
    pub txs: Vec<TransactionInfo>,
    pub sql: SqlDatabase,
    pub locals: HashMap<String, Value>,
    pub ancestors: Vec<RcState>,
    pub eventually: HashMap<usize, bool>,
}

impl State {
    pub fn hash(&self) -> HashableState {
        HashableState {
            pc: self.pc.clone(),
            global: self.sql.hash(),
            state: self.processes.clone(),
            locals: self
                .locals
                .iter()
                .map(|(l, r)| (l.clone(), r.clone()))
                .collect(),
            eventually: self.eventually.iter().map(|(l, r)| (*l, *r)).collect(),
        }
    }

    pub fn unlock_locks(&mut self) {
        let mut unlocks = vec![];
        'outer: for (i, s) in self.processes.iter().enumerate() {
            if let ProcessState::Locked(rid) = &s {
                for context in self.sql.transactions.values() {
                    if context.locks.contains(rid) {
                        continue 'outer;
                    }
                }

                unlocks.push(i);
            }
        }

        for unlock in unlocks {
            self.processes[unlock] = ProcessState::Running;
        }
    }

    pub fn find_deadlocks(&self) -> Option<HashSet<usize>> {
        for i in 0..self.processes.len() {
            let mut deq = VecDeque::from([i]);
            let mut cycle = HashSet::new();
            while let Some(x) = deq.pop_front() {
                if let ProcessState::Locked(rid) = self.processes[x] {
                    if cycle.contains(&x) {
                        return Some(cycle);
                    }
                    cycle.insert(x);
                    for (j, context) in &self.sql.transactions {
                        if context.locks.contains(&rid) {
                            for (pc, k) in self.txs.iter().enumerate() {
                                if k.id == *j {
                                    deq.push_back(pc);
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn unlock_latches(&mut self) {
        if self
            .processes
            .iter()
            .all(|w| w == &ProcessState::Latching || w == &ProcessState::Finished)
        {
            for w in self.processes.iter_mut() {
                if w == &ProcessState::Latching {
                    *w = ProcessState::Running;
                }
            }
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct RcState(Rc<RefCell<State>>);

impl RcState {
    pub fn new(state: State) -> RcState {
        RcState(Rc::new(RefCell::new(state)))
    }

    pub fn borrow(&self) -> Ref<'_, State> {
        RefCell::borrow(&self.0)
    }

    pub fn borrow_mut(&self) -> RefMut<'_, State> {
        RefCell::borrow_mut(&self.0)
    }
}

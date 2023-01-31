use crate::engine::Value;
use crate::parser::{Expression, IsolationLevel, Operator, Variable};
use crate::sql_engine::SqlEngineError::SqlTypeError;
use std::collections::HashMap;

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HashableRow {
    keys: Vec<String>,
    values: Vec<Value>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Row(pub HashMap<String, Value>);

impl Row {
    pub fn to_value(&self, columns: &[String]) -> Value {
        let mut res = vec![];
        for col in columns {
            res.push(self.0.get(col).unwrap().clone())
        }
        Value::Tuple(res)
    }

    pub fn keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.0.values().cloned().collect()
    }

    fn hashable(self) -> HashableRow {
        let (keys, values): (Vec<String>, Vec<Value>) = self.0.into_iter().unzip();
        HashableRow { keys, values }
    }
}

#[derive(PartialEq, Debug, Clone)]
enum Changes {
    Insert(String, Vec<Row>),
    Update(String, Row, String, Value),
}

#[derive(PartialEq, Debug, Clone)]
enum LockMode {
    ForUpdate,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Lock {
    row: Row,
    mode: LockMode,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Transaction {
    changes: Vec<Changes>,
    pub locks: Vec<Lock>,
}

impl Transaction {
    fn new() -> Self {
        Transaction {
            changes: vec![],
            locks: vec![],
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
enum SqlContext {
    Where {
        table: String,
        row: Row,
    },
    Update {
        tx: Option<TransactionId>,
        table: String,
        row: Row,
    },
}

#[derive(PartialEq, Debug, Clone)]
pub struct SqlDatabase {
    pub tables: HashMap<String, Vec<Row>>,
    pub transactions: HashMap<usize, Transaction>,
    tx: usize,
    sql_context: Option<SqlContext>,
}

impl SqlDatabase {
    pub fn hashable(&self) -> Vec<(String, Vec<HashableRow>)> {
        let mut res = vec![];
        for (name, rows) in &self.tables {
            res.push((
                name.clone(),
                rows.iter().map(|row| row.clone().hashable()).collect(),
            ));
        }
        res
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct TransactionId(pub usize);

#[derive(PartialEq, Debug, Clone)]
pub enum SqlEngineError {
    RowLockedError,
    SqlTypeError(Expression, String),
}

type Res<T> = Result<T, SqlEngineError>;
type Unit = Res<()>;

impl SqlDatabase {
    pub fn new() -> SqlDatabase {
        SqlDatabase {
            tables: Default::default(),
            transactions: Default::default(),
            tx: 0,
            sql_context: None,
        }
    }

    pub fn open_transaction(&mut self, _isolation: IsolationLevel) -> TransactionId {
        self.tx += 1;
        self.transactions.insert(self.tx, Transaction::new());

        TransactionId(self.tx)
    }

    fn interpret(&mut self, expression: &Expression) -> Res<Value> {
        match expression {
            Expression::Assignment(variable, expr) => {
                let value = self.interpret(expr)?;
                self.assign(variable.name.clone(), value)?;
                Ok(Value::Nil)
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => self.interpret_binary(left, operator, right),
            Expression::Var(variable) => {
                if let Some(sql_context) = &self.sql_context {
                    match sql_context {
                        SqlContext::Where { row, .. } => {
                            Ok(row.0.get(&variable.name).unwrap().clone())
                        }
                        _ => panic!(),
                    }
                } else {
                    panic!()
                }
            }
            Expression::Integer(i) => Ok(Value::Integer(*i)),
            Expression::Set(members) => {
                let mut res = vec![];
                for member in members {
                    res.push(self.interpret(member)?)
                }
                Ok(Value::Set(res))
            }
            Expression::Tuple(members) => {
                let mut res = vec![];
                for member in members {
                    res.push(self.interpret(member)?)
                }
                Ok(Value::Tuple(res))
            }
            Expression::Value(v) => Ok(v.clone()),
            Expression::Select { .. } => panic!(),
            Expression::Update { .. } => panic!(),
            Expression::Insert { .. } => panic!(),
        }
    }

    fn interpret_binary(
        &mut self,
        left: &Expression,
        operator: &Operator,
        right: &Expression,
    ) -> Res<Value> {
        match operator {
            Operator::Add => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left + right))
            }
            Operator::Multiply => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left * right))
            }
            Operator::Rem => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left % right))
            }
            Operator::Equal => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;
                Ok(Value::Bool(left == right))
            }
            Operator::Is => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;
                Ok(Value::Bool(left == right))
            }
            Operator::LessEqual => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left <= right))
            }
            Operator::Less => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Bool(left < right))
            }
            Operator::Included => {
                let left = self.interpret(left)?;
                let right = if let Value::Set(right) = self.interpret(right)? {
                    right
                } else {
                    todo!()
                };
                Ok(Value::Bool(right.contains(&left)))
            }
            Operator::And => {
                let left = if let Value::Bool(left) = self.interpret(left)? {
                    left
                } else {
                    todo!()
                };
                let right = if let Value::Bool(right) = self.interpret(right)? {
                    right
                } else {
                    todo!()
                };
                Ok(Value::Bool(left && right))
            }
        }
    }

    fn assert_integer(&mut self, expr: &Expression) -> Res<i16> {
        if let Value::Integer(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "integer".to_string()))
        }
    }

    fn assert_set(&mut self, expr: &Expression) -> Res<Vec<Value>> {
        if let Value::Set(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "set".to_string()))
        }
    }

    fn assert_bool(&mut self, expr: &Expression) -> Res<bool> {
        if let Value::Bool(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "bool".to_string()))
        }
    }

    fn assert_tuple(&mut self, expr: &Expression) -> Res<Vec<Value>> {
        if let Value::Tuple(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "tuple".to_string()))
        }
    }

    pub fn insert_in_table(
        &mut self,
        tx: &Option<TransactionId>,
        table: &str,
        tuples: Vec<Vec<Value>>,
        columns: &[Variable],
    ) {
        let mut rows = vec![];
        for tuple in tuples {
            let mut new_row = HashMap::new();
            for (i, col) in columns.iter().enumerate() {
                new_row.insert(col.name.clone(), tuple[i].clone());
            }
            rows.push(Row(new_row))
        }

        if let Some(tx) = tx {
            let transaction = self.transactions.get_mut(&tx.0).unwrap();
            transaction
                .changes
                .push(Changes::Insert(table.to_string(), rows));
        } else {
            let table = self.tables.entry(table.to_string()).or_default();
            for row in rows {
                table.push(row);
            }
        }
    }

    pub fn update_in_table(
        &mut self,
        tx: &Option<TransactionId>,
        table: &String,
        update: &Expression,
        condition: &Option<Box<Expression>>,
    ) -> Unit {
        let rows = self.rows(tx, table);

        for row in rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: table.clone(),
                });
                if self.interpret(cond)? == Value::Bool(true) {
                    self.sql_context = Some(SqlContext::Update {
                        tx: *tx,
                        row: row.clone(),
                        table: table.clone(),
                    });
                    self.interpret(update)?;
                }
            } else {
                self.sql_context = Some(SqlContext::Update {
                    tx: *tx,
                    row: row.clone(),
                    table: table.clone(),
                });
                self.interpret(update)?;
            }
            self.sql_context = None;
        }
        Ok(())
    }

    pub fn select_in_table(
        &mut self,
        tx: &Option<TransactionId>,
        columns: &[Variable],
        from: &String,
        condition: &Option<Box<Expression>>,
        locking: bool,
    ) -> Res<Vec<Value>> {
        let columns: Vec<_> = columns.iter().map(|v| v.name.clone()).collect();
        let rows = self.rows(tx, from);

        let mut res = vec![];
        for row in &rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: from.clone(),
                });
                if locking {
                    if self.is_locked(
                        &tx.unwrap_or(TransactionId(usize::MAX)),
                        row,
                        LockMode::ForUpdate,
                    ) {
                        return Err(SqlEngineError::RowLockedError);
                    } else if let Some(tx) = tx {
                        let transaction = self.transactions.get_mut(&tx.0).unwrap();
                        transaction.locks.push(Lock {
                            row: row.clone(),
                            mode: LockMode::ForUpdate,
                        });
                    }
                }
                if self.interpret(cond)? == Value::Bool(true) {
                    res.push(row.to_value(&columns))
                }
                self.sql_context = None;
            } else {
                res.push(row.to_value(&columns))
            }
        }

        Ok(res)
    }

    fn rows(&self, tx: &Option<TransactionId>, table: &String) -> Vec<Row> {
        let mut rows = self.tables.get(table).cloned().unwrap_or_default();

        if let Some(tx) = tx {
            let transaction = self.transactions.get(&tx.0).unwrap();
            for changes in &transaction.changes {
                match changes {
                    Changes::Insert(insert_table, insert_rows) => {
                        if insert_table == table {
                            rows.extend_from_slice(insert_rows);
                        }
                    }
                    Changes::Update(_, _, _, _) => {}
                }
            }
        }
        rows
    }

    pub fn commit(&mut self, tx: &TransactionId) {
        let x = self.transactions.remove(&tx.0).unwrap();
        for change in x.changes {
            match change {
                Changes::Insert(table, rows) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    table.extend_from_slice(&rows);
                }
                Changes::Update(table, row, col, value) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    for r in table {
                        if r == &row {
                            r.0.insert(col.clone(), value.clone());
                        }
                    }
                }
            }
        }
    }

    pub fn abort(&mut self, tx: &TransactionId) {
        self.transactions.remove(&tx.0).unwrap();
    }

    fn assign(&mut self, name: String, value: Value) -> Unit {
        if let Some(sql_context) = &self.sql_context {
            match sql_context {
                SqlContext::Update { tx, table, row } => {
                    if let Some(tx) = tx {
                        if self.is_locked(tx, row, LockMode::ForUpdate) {
                            return Err(SqlEngineError::RowLockedError);
                        } else {
                            let transaction = self.transactions.get_mut(&tx.0).unwrap();
                            transaction.locks.push(Lock {
                                row: row.clone(),
                                mode: LockMode::ForUpdate,
                            });
                            transaction.changes.push(Changes::Update(
                                table.clone(),
                                row.clone(),
                                name,
                                value,
                            ));
                        }
                    } else {
                        let table = self.tables.entry(table.clone()).or_default();
                        for r in table {
                            if r == row {
                                r.0.insert(name.clone(), value.clone());
                            }
                        }
                    }
                }
                _ => panic!(),
            }
        }

        Ok(())
    }

    fn is_locked(&self, tx: &TransactionId, row: &Row, mode: LockMode) -> bool {
        for (id, t) in &self.transactions {
            if *id != tx.0 && t.locks.iter().any(|l| l.mode == mode && &l.row == row) {
                return true;
            }
        }
        false
    }
}

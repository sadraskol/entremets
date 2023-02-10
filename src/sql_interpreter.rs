use std::collections::HashMap;

use crate::engine::Value;
use crate::parser::{IsolationLevel, SqlExpression, SqlOperator, Variable};
use crate::sql_interpreter::SqlEngineError::{SqlTypeError, UnknownVariable};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HashableRow {
    keys: Vec<String>,
    values: Vec<Value>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Row(pub HashMap<String, Value>, RowId);

impl Row {
    pub fn to_value(&self, columns: &[String]) -> Value {
        if columns.len() == 1 {
            return self.0.get(&columns[0]).unwrap().clone();
        } else {
            let mut res = vec![];
            for col in columns {
                res.push(self.0.get(col).unwrap().clone())
            }
            Value::Tuple(res)
        }
    }

    pub fn keys(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.0.values().cloned().collect()
    }

    fn hash(self) -> HashableRow {
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
pub struct TransactionContext {
    changes: Vec<Changes>,
    pub locks: Vec<RowId>,
}

impl TransactionContext {
    fn new() -> Self {
        TransactionContext {
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
    pub cur_tx: Option<TransactionId>,
    pub tables: HashMap<String, Vec<Row>>,
    pub transactions: HashMap<TransactionId, TransactionContext>,
    tx: TransactionId,
    rid: RowId,
    sql_context: Option<SqlContext>,
}

impl SqlDatabase {
    pub fn hash(&self) -> Vec<(String, Vec<HashableRow>)> {
        let mut res = vec![];
        for (name, rows) in &self.tables {
            res.push((
                name.clone(),
                rows.iter().map(|row| row.clone().hash()).collect(),
            ));
        }
        res
    }
}

#[derive(PartialEq, Debug, Clone, Copy, Eq, Hash)]
pub struct TransactionId(pub usize);

impl TransactionId {
    fn increment(&mut self) -> TransactionId {
        self.0 += 1;
        TransactionId(self.0)
    }
}

#[derive(PartialEq, Debug, Clone, Copy, Hash, Eq)]
pub struct RowId(usize);

impl RowId {
    fn increment(&mut self) -> RowId {
        self.0 += 1;
        RowId(self.0)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum SqlEngineError {
    RowLockedError(RowId),
    SqlTypeError(SqlExpression, String),
    TransactionAborted,
    UnknownVariable(String),
}

type Res<T> = Result<T, SqlEngineError>;
type Unit = Res<()>;

impl SqlDatabase {
    pub fn new() -> SqlDatabase {
        SqlDatabase {
            cur_tx: None,
            tables: Default::default(),
            transactions: Default::default(),
            tx: TransactionId(0),
            sql_context: None,
            rid: RowId(0),
        }
    }

    pub fn open_transaction(&mut self, _isolation: IsolationLevel) -> TransactionId {
        let new_tx = self.tx.increment();
        self.transactions.insert(new_tx, TransactionContext::new());

        new_tx
    }

    pub fn interpret(&mut self, expr: &SqlExpression) -> Res<Value> {
        match expr {
            SqlExpression::Select {
                columns,
                from,
                condition,
                locking,
            } => self.interpret_select(columns, from, condition, *locking),
            SqlExpression::Update {
                relation,
                update,
                condition,
            } => self.interpret_update(relation, update, condition),
            SqlExpression::Insert {
                relation,
                columns,
                values,
            } => self.interpret_insert(relation, columns, values),
            SqlExpression::Binary {
                left,
                operator,
                right,
            } => self.interpret_binary(left, operator, right),
            SqlExpression::Assignment(variable, expr) => {
                let value = self.interpret(expr)?;
                self.assign(variable.name.clone(), value)?;
                Ok(Value::Nil)
            }
            SqlExpression::Integer(i) => Ok(Value::Integer(*i)),
            SqlExpression::Tuple(values) => {
                let mut res = vec![];
                for value in values {
                    res.push(self.interpret(value)?);
                }
                Ok(Value::Tuple(res))
            }
            SqlExpression::Var(var) => {
                if let Some(SqlContext::Where { row, .. }) = &self.sql_context {
                    Ok(row.0.get(&var.name).unwrap().clone())
                } else {
                    Err(UnknownVariable(var.name.clone()))
                }
            }
            SqlExpression::UpVariable(_) => panic!("UpVariable should not be interpreted directly"),
            SqlExpression::Value(value) => Ok(value.clone()),
            SqlExpression::Set(members) => {
                let mut res = vec![];
                for member in members {
                    res.push(self.interpret(member)?);
                }
                Ok(Value::Set(res))
            }
        }
    }

    fn interpret_binary(
        &mut self,
        left: &SqlExpression,
        operator: &SqlOperator,
        right: &SqlExpression,
    ) -> Res<Value> {
        match operator {
            SqlOperator::Add => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left + right))
            }
            SqlOperator::Multiply => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left * right))
            }
            SqlOperator::Rem => {
                let left = self.assert_integer(left)?;
                let right = self.assert_integer(right)?;
                Ok(Value::Integer(left % right))
            }
            SqlOperator::Equal => {
                let left = self.interpret(left)?;
                let right = self.interpret(right)?;
                Ok(Value::Bool(left == right))
            }
            SqlOperator::And => {
                let left = self.assert_bool(left)?;
                let right = self.assert_bool(right)?;
                Ok(Value::Bool(left && right))
            }
            SqlOperator::In => {
                let left = self.interpret(left)?;
                let right = self.assert_set(right)?;
                Ok(Value::Bool(right.contains(&left)))
            }
        }
    }

    fn interpret_insert(
        &mut self,
        relation: &Variable,
        columns: &[Variable],
        exprs: &[SqlExpression],
    ) -> Res<Value> {
        let mut values = vec![];
        for expr in exprs {
            values.push(self.assert_tuple(expr)?)
        }

        let table = &relation.name;
        let mut rows = vec![];
        for tuple in values {
            let mut new_row = HashMap::new();
            for (i, col) in columns.iter().enumerate() {
                new_row.insert(col.name.clone(), tuple[i].clone());
            }
            rows.push(Row(new_row, self.rid.increment()))
        }

        if let Some(tx) = self.cur_tx {
            let transaction = self.transactions.get_mut(&tx).unwrap();
            transaction
                .changes
                .push(Changes::Insert(table.to_string(), rows));
        } else {
            let table = self.tables.entry(table.to_string()).or_default();
            for row in rows {
                table.push(row);
            }
        }

        Ok(Value::Nil)
    }

    fn interpret_update(
        &mut self,
        relation: &Variable,
        update: &SqlExpression,
        condition: &Option<Box<SqlExpression>>,
    ) -> Res<Value> {
        let table = &relation.name;
        let rows = self.rows(&self.cur_tx, table);

        let mut mutated = false;
        for row in rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: table.clone(),
                });
                if self.interpret(cond)? == Value::Bool(true) {
                    self.sql_context = Some(SqlContext::Update {
                        tx: self.cur_tx,
                        row: row.clone(),
                        table: table.clone(),
                    });
                    self.interpret(update)?;
                    mutated = true;
                }
            } else {
                self.sql_context = Some(SqlContext::Update {
                    tx: self.cur_tx,
                    row: row.clone(),
                    table: table.clone(),
                });
                self.interpret(update)?;
                mutated = true;
            }
            self.sql_context = None;
        }

        if !mutated {
            Err(SqlEngineError::TransactionAborted)
        } else {
            Ok(Value::Nil)
        }
    }

    fn interpret_select(
        &mut self,
        columns: &[Variable],
        from: &Variable,
        condition: &Option<Box<SqlExpression>>,
        locking: bool,
    ) -> Res<Value> {
        let columns: Vec<_> = columns.iter().map(|v| v.name.clone()).collect();
        let rows = self.rows(&self.cur_tx, &from.name);

        let mut res = vec![];
        for row in &rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: from.name.clone(),
                });
                if locking {
                    self.check_locked_row(&self.cur_tx.unwrap_or(TransactionId(usize::MAX)), row)?;
                    if let Some(tx) = &self.cur_tx {
                        let transaction = self.transactions.get_mut(tx).unwrap();
                        transaction.locks.push(row.1);
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

        if res.len() == 1 {
            return Ok(res[0].clone());
        }

        Ok(Value::Set(res))
    }

    fn assert_integer(&mut self, expr: &SqlExpression) -> Res<i16> {
        if let Value::Integer(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "integer".to_string()))
        }
    }

    fn assert_bool(&mut self, expr: &SqlExpression) -> Res<bool> {
        if let Value::Bool(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "bool".to_string()))
        }
    }

    fn assert_tuple(&mut self, expr: &SqlExpression) -> Res<Vec<Value>> {
        if let Value::Tuple(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "tuple".to_string()))
        }
    }

    fn assert_set(&mut self, expr: &SqlExpression) -> Res<Vec<Value>> {
        if let Value::Set(value) = self.interpret(expr)? {
            Ok(value)
        } else {
            Err(SqlTypeError(expr.clone(), "set".to_string()))
        }
    }

    fn rows(&self, tx: &Option<TransactionId>, table: &String) -> Vec<Row> {
        let mut rows = self.tables.get(table).cloned().unwrap_or_default();

        if let Some(tx) = tx {
            let transaction = self.transactions.get(tx).unwrap();
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
        let tx = self.transactions.remove(tx).unwrap();
        for change in tx.changes {
            match change {
                Changes::Insert(table, rows) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    table.extend_from_slice(&rows);
                }
                Changes::Update(table, row, col, value) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    for r in table {
                        if r.1 == row.1 {
                            r.0.insert(col.clone(), value.clone());
                        }
                    }
                }
            }
        }
    }

    pub fn abort(&mut self, tx: &TransactionId) {
        self.transactions.remove(tx).unwrap();
    }

    fn assign(&mut self, name: String, value: Value) -> Unit {
        if let Some(sql_context) = self.sql_context.clone() {
            match sql_context {
                SqlContext::Update { tx, table, row } => {
                    if let Some(tx) = tx {
                        self.check_locked_row(&tx, &row)?;

                        let transaction = self.transactions.get_mut(&tx).unwrap();
                        transaction.locks.push(row.1);
                        transaction
                            .changes
                            .push(Changes::Update(table, row, name, value));
                    } else {
                        let table = self.tables.entry(table).or_default();
                        for r in table {
                            if r.1 == row.1 {
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

    fn check_locked_row(&self, tx: &TransactionId, row: &Row) -> Unit {
        for (id, t) in &self.transactions {
            if id != tx && t.locks.contains(&row.1) {
                return Err(SqlEngineError::RowLockedError(row.1));
            }
        }
        Ok(())
    }
}

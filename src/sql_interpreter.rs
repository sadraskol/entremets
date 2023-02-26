use std::collections::HashMap;

use crate::engine::Value;
use crate::parser::{IsolationLevel, SelectClause, SqlExpression, SqlOperator, Variable};
use crate::sql_interpreter::SqlEngineError::{SqlTypeError, UnknownVariable};

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct HashableRow {
    keys: Vec<String>,
    values: Vec<Value>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Row {
    pub tuples: HashMap<String, Value>,
    rid: RowId,
}

impl Row {
    pub fn to_value(&self, columns: &[SelectClause]) -> Value {
        if columns.len() == 1 {
            columns[0].extract(&self.tuples)
        } else {
            let mut res = vec![];
            for col in columns {
                res.push(col.extract(&self.tuples))
            }
            Value::Tuple(res)
        }
    }

    pub fn keys(&self) -> Vec<String> {
        self.tuples.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.tuples.values().cloned().collect()
    }

    fn hash(self) -> HashableRow {
        let (keys, values): (Vec<String>, Vec<Value>) = self.tuples.into_iter().unzip();
        HashableRow { keys, values }
    }
}

#[derive(PartialEq, Debug, Clone)]
enum Changes {
    Insert(String, Row),
    Update(String, Row, String, Value),
}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum Lock {
    RowUpdate(RowId),
    Unique(String, String, Value),
}

#[derive(PartialEq, Debug, Clone)]
pub struct TransactionContext {
    changes: Vec<Changes>,
    pub locks: Vec<Lock>,
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
        tx: TransactionId,
        table: String,
        row: Row,
    },
}

#[derive(PartialEq, Default, Debug, Clone)]
pub struct Table {
    pub rows: Vec<Row>,
    pub unique: Vec<String>,
}

#[derive(PartialEq, Debug, Clone)]
pub struct SqlDatabase {
    pub cur_tx: TransactionId,
    pub tables: HashMap<String, Table>,
    pub transactions: HashMap<TransactionId, TransactionContext>,
    tx: TransactionId,
    rid: RowId,
    sql_context: Option<SqlContext>,
}

impl SqlDatabase {
    pub fn hash(&self) -> Vec<(String, Vec<HashableRow>)> {
        let mut res = vec![];
        for (name, table) in &self.tables {
            res.push((
                name.clone(),
                table.rows.iter().map(|row| row.clone().hash()).collect(),
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
    Locked(Lock),
    SqlTypeError(SqlExpression, String),
    UnicityViolation,
    UnknownVariable(String),
}

type Res<T> = Result<T, SqlEngineError>;
type Unit = Res<()>;

impl SqlDatabase {
    pub fn new() -> SqlDatabase {
        SqlDatabase {
            cur_tx: TransactionId(usize::MAX),
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

    pub fn execute(&mut self, expr: &SqlExpression, opt_tx: Option<TransactionId>) -> Res<Value> {
        self.cur_tx = if let Some(tx) = opt_tx {
            tx
        } else {
            self.open_transaction(IsolationLevel::ReadCommitted)
        };

        let res = self.interpret(expr)?;

        if opt_tx.is_none() {
            self.commit(&self.cur_tx.clone());
        }

        Ok(res)
    }

    fn interpret(&mut self, expr: &SqlExpression) -> Res<Value> {
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
                    Ok(row.tuples.get(&var.name).unwrap().clone())
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
            SqlExpression::Create { relation, column } => {
                let table = self.tables.entry(relation.name.clone()).or_default();
                table.unique.push(column.name.clone());
                Ok(Value::Nil)
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
        for tuple in values {
            let mut new_row = HashMap::new();
            for (i, col) in columns.iter().enumerate() {
                self.check_unique_values(&self.cur_tx, table, &col.name, &tuple[i])?;
                new_row.insert(col.name.clone(), tuple[i].clone());
            }

            let transaction = self.transactions.get_mut(&self.cur_tx).unwrap();

            if let Some(t) = self.tables.get(table) {
                for unique in &t.unique {
                    transaction.locks.push(Lock::Unique(
                        table.clone(),
                        unique.clone(),
                        new_row.get(unique).unwrap().clone(),
                    ));
                }
            }
            transaction.changes.push(Changes::Insert(
                table.to_string(),
                Row {
                    tuples: new_row,
                    rid: self.rid.increment(),
                },
            ));
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

        let mut mutated = 0;
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
                    mutated += 1;
                }
            } else {
                self.sql_context = Some(SqlContext::Update {
                    tx: self.cur_tx,
                    row: row.clone(),
                    table: table.clone(),
                });
                self.interpret(update)?;
                mutated += 1;
            }
            self.sql_context = None;
        }

        Ok(Value::Integer(mutated))
    }

    fn interpret_select(
        &mut self,
        columns: &[SelectClause],
        from: &Variable,
        condition: &Option<Box<SqlExpression>>,
        for_update: bool,
    ) -> Res<Value> {
        let rows = self.rows(&self.cur_tx, &from.name);

        let mut res = vec![];
        for row in &rows {
            if let Some(cond) = condition {
                self.sql_context = Some(SqlContext::Where {
                    row: row.clone(),
                    table: from.name.clone(),
                });
                if for_update {
                    self.check_locked_row(&self.cur_tx, row)?;
                    let transaction = self.transactions.get_mut(&self.cur_tx).unwrap();
                    transaction.locks.push(Lock::RowUpdate(row.rid));
                }
                if self.interpret(cond)? == Value::Bool(true) {
                    res.push(row.to_value(columns))
                }
                self.sql_context = None;
            } else {
                res.push(row.to_value(columns))
            }
        }

        if columns.iter().any(|col| matches!(col, SelectClause::Count)) {
            return Ok(Value::Integer(res.len() as i16));
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

    fn rows(&self, tx: &TransactionId, table_name: &String) -> Vec<Row> {
        let mut table = self.tables.get(table_name).cloned().unwrap_or_default();

        let transaction = self.transactions.get(tx).unwrap();
        for changes in &transaction.changes {
            match changes {
                Changes::Insert(insert_table, insert_row) => {
                    if insert_table == table_name {
                        table.rows.push(insert_row.clone());
                    }
                }
                Changes::Update(_, _, _, _) => {}
            }
        }
        table.rows
    }

    pub fn commit(&mut self, tx: &TransactionId) {
        let tx = self.transactions.remove(tx).unwrap();
        for change in tx.changes {
            match change {
                Changes::Insert(table, row) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    table.rows.push(row);
                }
                Changes::Update(table, row, col, value) => {
                    let table = self.tables.entry(table.clone()).or_default();
                    for r in &mut table.rows {
                        if r.rid == row.rid {
                            r.tuples.insert(col.clone(), value.clone());
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
                    self.check_locked_row(&tx, &row)?;
                    self.check_unique_values(&tx, &table, &name, &value)?;

                    let transaction = self.transactions.get_mut(&tx).unwrap();

                    let t = self.tables.get(&table).unwrap();
                    if t.unique.contains(&name) {
                        transaction.locks.push(Lock::Unique(
                            table.clone(),
                            name.clone(),
                            value.clone(),
                        ));
                    }
                    transaction.locks.push(Lock::RowUpdate(row.rid));
                    transaction
                        .changes
                        .push(Changes::Update(table, row, name, value));
                }
                _ => panic!(),
            }
        }

        Ok(())
    }

    fn check_locked_row(&self, tx: &TransactionId, row: &Row) -> Unit {
        for (id, t) in &self.transactions {
            let lock = Lock::RowUpdate(row.rid);
            if id != tx && t.locks.contains(&lock) {
                return Err(SqlEngineError::Locked(lock));
            }
        }
        Ok(())
    }

    fn check_unique_values(
        &self,
        tx: &TransactionId,
        table: &str,
        name: &str,
        value: &Value,
    ) -> Unit {
        for (id, t) in &self.transactions {
            let lock = Lock::Unique(table.to_string(), name.to_string(), value.clone());
            if id != tx && t.locks.contains(&lock) {
                return Err(SqlEngineError::Locked(lock));
            }
        }

        if let Some(t) = self.tables.get(table) {
            for existing in &t.rows {
                if existing.tuples.get(name) == Some(value) {
                    return Err(SqlEngineError::UnicityViolation);
                }
            }
        }
        Ok(())
    }
}

use std::collections::HashMap;
use crate::engine::Value;

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
pub struct SqlDatabase {
    pub(crate) tables: HashMap<String, Vec<Row>>,
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

impl SqlDatabase {
    pub fn new() -> SqlDatabase {
        SqlDatabase {
            tables: Default::default(),
        }
    }
}

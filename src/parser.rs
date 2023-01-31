use crate::engine::Value;
use crate::parser::Expression::{Insert, Update};
use crate::parser::ParserError::Unexpected;
use crate::scanner::{Position, Scanner, ScannerError, Token, TokenKind};
use std::mem;
use std::num::ParseIntError;
use std::ops::Deref;
use std::str::FromStr;

struct Lexeme<T> {
    t: T,
    position: Position,
    lexeme: String,
}

impl<T> Deref for Lexeme<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Begin(IsolationLevel, Option<Variable>),
    Commit,
    Abort,
    Expression(Expression),
    Latch,

    Always(Expression),
    Never(Expression),
    Eventually(Expression),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsolationLevel {
    ReadCommitted,
}

#[derive(PartialEq, Debug, Clone)]
pub struct Variable {
    pub name: String,
}

#[derive(PartialEq, Debug, Clone)]
pub enum Expression {
    Select {
        columns: Vec<Variable>,
        from: Variable,
        condition: Option<Box<Expression>>,
    },
    Update {
        relation: Variable,
        update: Box<Expression>,
        condition: Option<Box<Expression>>,
    },
    Insert {
        relation: Variable,
        columns: Vec<Variable>,
        values: Vec<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: Operator,
        right: Box<Expression>,
    },
    Assignment(Variable, Box<Expression>),
    Var(Variable),
    Integer(i16),
    Set(Vec<Expression>),
    Tuple(Vec<Expression>),

    // this enum is only here for convenience for the sql interpreter
    Value(Value),
}

#[derive(PartialEq, Debug, Clone)]
pub enum Operator {
    Add,
    Multiply,
    Rem,
    Equal,
    Is,
    LessEqual,
    Less,
    Included,
    And,
}

pub struct Parser {
    scanner: Scanner,
    manual_commit: bool,
    previous: Token,
    current: Token,
    result: Mets,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParserError {
    ParseInt(ParseIntError),
    Scanner(ScannerError),
    Uninitialized,
    Unexpected(String),
}

impl From<ScannerError> for ParserError {
    fn from(value: ScannerError) -> Self {
        ParserError::Scanner(value)
    }
}

impl From<ParseIntError> for ParserError {
    fn from(value: ParseIntError) -> Self {
        ParserError::ParseInt(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mets {
    pub init: Vec<Statement>,
    pub processes: Vec<Vec<Statement>>,
    pub properties: Vec<Statement>,
}

pub type Res<T> = Result<T, ParserError>;
type Unit = Res<()>;

impl Parser {
    pub fn new(source: String) -> Self {
        Parser {
            scanner: Scanner::new(source),
            manual_commit: false,
            previous: Token::uninitialized(),
            current: Token::uninitialized(),
            result: Mets {
                init: vec![],
                processes: vec![],
                properties: vec![],
            },
        }
    }

    pub fn compile(mut self) -> Result<Mets, String> {
        match self.private_compile() {
            Ok(_) => Ok(self.result),
            Err(err) => Err(format!(
                "Error at position {:?}: {:?}
                previous: {:?}
                current: {:?}",
                self.current.position, err, self.previous, self.current
            )),
        }
    }

    fn private_compile(&mut self) -> Unit {
        self.advance()?;
        self.skip_newlines()?;
        while !self.matches(TokenKind::Eof)? {
            self.declaration()?;
        }
        self.consume(TokenKind::Eof, "Expect end of expression")
    }

    fn advance(&mut self) -> Unit {
        mem::swap(&mut self.previous, &mut self.current);
        self.current = self.scanner.scan_token()?;

        Ok(())
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.current.kind == kind
    }

    fn matches(&mut self, kind: TokenKind) -> Res<bool> {
        Ok(if self.current.kind == kind {
            self.advance()?;
            true
        } else {
            false
        })
    }

    fn consume(&mut self, kind: TokenKind, expected: &str) -> Unit {
        if self.current.kind == kind {
            self.advance()
        } else {
            Err(Unexpected(expected.to_string()))
        }
    }

    fn declaration(&mut self) -> Unit {
        if self.matches(TokenKind::Process)? {
            self.process_declaration()
        } else if self.matches(TokenKind::Init)? {
            self.init_declaration()
        } else if self.matches(TokenKind::Property)? {
            self.property_declaration()
        } else {
            Err(Unexpected(format!(
                "Expected either process, init or property. Parsed {:?} instead",
                self.current.kind
            )))
        }
    }

    fn init_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expect do after init declaration")?;
        self.consume(TokenKind::Newline, "Expect newline after init declaration")?;

        let mut statements = vec![];
        while self.current.kind != TokenKind::End {
            self.statement(&mut statements)?;
        }
        self.result.init = statements;

        self.consume(TokenKind::End, "Expect end at the end of init declaration")?;

        self.end_line()
    }

    fn process_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expect do after process declaration")?;
        self.consume(
            TokenKind::Newline,
            "Expect newline after process declaration",
        )?;

        let mut statements = vec![];
        while self.current.kind != TokenKind::End {
            self.statement(&mut statements)?;
        }
        self.result.processes.push(statements);

        self.consume(
            TokenKind::End,
            "Expect end at the end of process declaration",
        )?;

        self.end_line()
    }

    fn end_line(&mut self) -> Unit {
        if !self.matches(TokenKind::Eof)? {
            self.consume(TokenKind::Newline, "Expect newline after declaration")?;
        }
        self.skip_newlines()
    }

    fn property_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Equal, "Expect = after property declaration")?;

        let mut statements = vec![];
        self.statement(&mut statements)?;
        self.result.properties.push(statements.remove(0));

        Ok(())
    }

    fn statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        if self.matches(TokenKind::Let)? {
            self.assignment_statement(writer)?;
        } else if self.matches(TokenKind::Transaction)? {
            self.transaction_statement(writer)?;
        } else if self.matches(TokenKind::Begin)? {
            self.begin_statement(writer)?;
        } else if self.matches(TokenKind::Commit)? {
            self.commit_statement(writer)?;
        } else if self.matches(TokenKind::Abort)? {
            self.abort_statement(writer)?;
        } else if self.matches(TokenKind::Latch)? {
            self.latch_statement(writer)?;
        } else if self.matches(TokenKind::Always)? {
            self.always_statement(writer)?;
        } else if self.matches(TokenKind::Never)? {
            self.never_statement(writer)?;
        } else if self.matches(TokenKind::Eventually)? {
            self.eventually_statement(writer)?;
        } else {
            self.expression_statement(writer)?;
        };

        self.end_line()
    }

    fn assignment_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let expr = self.assignment()?;

        writer.push(Statement::Expression(expr));
        Ok(())
    }

    fn transaction_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let tx_name = self.parse_variable("Expect transaction name")?;
        self.consume(
            TokenKind::Identifier,
            "Expect isolation level after transaction name",
        )?;

        match self.previous.lexeme.as_str() {
            "read_committed" => {
                self.consume(TokenKind::Do, "Expect block after transaction statement")?;
                self.end_line()?;

                writer.push(Statement::Begin(
                    IsolationLevel::ReadCommitted,
                    Some(tx_name),
                ));
                self.manual_commit = false;

                while self.current.kind != TokenKind::End {
                    self.statement(writer)?;
                }

                self.consume(TokenKind::End, "Expect to close transaction block")?;

                if !self.manual_commit {
                    writer.push(Statement::Commit);
                }
                Ok(())
            }
            _ => Err(Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn parse_variable(&mut self, expected: &str) -> Res<Variable> {
        self.consume(TokenKind::Identifier, expected)?;

        self.make_variable()
    }

    fn begin_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(TokenKind::Identifier, "Expect isolation level after begin")?;

        match self.previous.lexeme.as_str() {
            "read_committed" => {
                writer.push(Statement::Begin(IsolationLevel::ReadCommitted, None));
                Ok(())
            }
            _ => Err(Unexpected(
                "Expected following isolation level: read_committed".to_string(),
            )),
        }
    }

    fn commit_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Commit);
        self.manual_commit = true;
        Ok(())
    }

    fn abort_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Abort);
        self.manual_commit = true;
        Ok(())
    }

    fn latch_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        writer.push(Statement::Latch);
        Ok(())
    }

    fn always_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(TokenKind::LeftBracket, "Expect [ to open always statement")?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightBracket,
            "Expect ] to close always statement",
        )?;

        writer.push(Statement::Always(expr));
        Ok(())
    }

    fn never_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(TokenKind::LeftBracket, "Expect [ to open never statement")?;

        let expr = self.expression()?;

        self.consume(TokenKind::RightBracket, "Expect ] to close never statement")?;
        writer.push(Statement::Never(expr));
        Ok(())
    }

    fn eventually_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        self.consume(
            TokenKind::LeftCarret,
            "Expect < to open eventually statement",
        )?;

        let expr = self.expression()?;

        self.consume(
            TokenKind::RightCarret,
            "Expect > to close eventually statement",
        )?;
        writer.push(Statement::Eventually(expr));
        Ok(())
    }

    fn expression_statement(&mut self, writer: &mut Vec<Statement>) -> Unit {
        let expr = self.expression()?;
        writer.push(Statement::Expression(expr));
        Ok(())
    }

    fn expression(&mut self) -> Res<Expression> {
        self.assignment()
    }

    fn assignment(&mut self) -> Res<Expression> {
        let mut expr = self.included()?;

        if self.matches(TokenKind::ColonEqual)? {
            let name = if let Expression::Var(name) = expr {
                name
            } else {
                return Err(Unexpected(format!(
                    "Expected variable before := assignment at {:?}",
                    self.previous
                )));
            };
            let value = self.assignment()?;
            expr = Expression::Assignment(name, Box::new(value));
        }

        Ok(expr)
    }

    fn included(&mut self) -> Res<Expression> {
        let mut expr = self.is()?;

        while self.matches(TokenKind::In)? {
            let right = self.is()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Included,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn is(&mut self) -> Res<Expression> {
        let mut expr = self.and()?;

        while self.matches(TokenKind::Is)? {
            let right = self.and()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Is,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn and(&mut self) -> Res<Expression> {
        let mut expr = self.equality()?;

        while self.matches(TokenKind::And)? {
            let right = self.equality()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::And,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn equality(&mut self) -> Res<Expression> {
        let mut expr = self.comparison()?;

        while self.matches(TokenKind::Equal)? {
            let right = self.comparison()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Equal,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn comparison(&mut self) -> Res<Expression> {
        let mut expr = self.term()?;

        while self.matches(TokenKind::LeftCarret)? || self.matches(TokenKind::LessEqual)? {
            let operator = match self.previous.lexeme.as_str() {
                "<" => Ok(Operator::Less),
                "<=" => Ok(Operator::LessEqual),
                _ => Err(Unexpected(format!(
                    "unknown comparison operator: {}",
                    self.previous.lexeme
                ))),
            }?;
            let right = self.term()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn term(&mut self) -> Res<Expression> {
        let mut expr = self.factor()?;

        while self.matches(TokenKind::Plus)? {
            let right = self.factor()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Add,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn factor(&mut self) -> Res<Expression> {
        let mut expr = self.remainder()?;

        while self.matches(TokenKind::Star)? {
            let right = self.remainder()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Multiply,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn remainder(&mut self) -> Res<Expression> {
        let mut expr = self.primary()?;

        while self.matches(TokenKind::Percent)? {
            let right = self.primary()?;
            expr = Expression::Binary {
                left: Box::new(expr),
                operator: Operator::Rem,
                right: Box::new(right),
            }
        }

        Ok(expr)
    }

    fn primary(&mut self) -> Res<Expression> {
        if self.matches(TokenKind::Number)? {
            self.number()
        } else if self.matches(TokenKind::LeftBrace)? {
            self.set()
        } else if self.matches(TokenKind::LeftParen)? {
            self.tuple()
        } else if self.matches(TokenKind::Identifier)? {
            self.variable()
        } else if self.matches(TokenKind::Select)? {
            self.select()
        } else if self.matches(TokenKind::Update)? {
            self.update()
        } else if self.matches(TokenKind::Insert)? {
            self.insert()
        } else {
            Err(Unexpected(format!(
                "Expected expression, got: {:?}",
                self.current
            )))
        }
    }

    fn variable(&mut self) -> Res<Expression> {
        Ok(Expression::Var(self.make_variable()?))
    }

    fn number(&mut self) -> Res<Expression> {
        let i = i16::from_str(&self.previous.lexeme)?;
        Ok(Expression::Integer(i))
    }

    fn set(&mut self) -> Res<Expression> {
        self.skip_newlines()?;
        let mut members = vec![];
        if !self.check(TokenKind::RightBrace) {
            loop {
                let member = self.expression()?;
                members.push(member);

                if !self.matches(TokenKind::Comma)? {
                    break;
                }
                self.skip_newlines()?;
            }
            self.skip_newlines()?;
        }
        self.consume(TokenKind::RightBrace, "Expect } to close a set expression")?;

        Ok(Expression::Set(members))
    }

    fn tuple(&mut self) -> Res<Expression> {
        let mut members = vec![];
        loop {
            let member = self.expression()?;
            members.push(member);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(TokenKind::RightParen, "Expect closing ) for tuple")?;

        Ok(Expression::Tuple(members))
    }

    fn select(&mut self) -> Res<Expression> {
        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(TokenKind::From, "Expect from clause")?;

        self.consume(TokenKind::Identifier, "Expect relation for select from")?;
        let from = self.make_variable()?;

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            let expr = self.and()?;
            condition = Some(Box::new(expr));
        }

        Ok(Expression::Select {
            columns,
            from,
            condition,
        })
    }

    fn update(&mut self) -> Res<Expression> {
        self.consume(TokenKind::Identifier, "expect relation for update")?;
        let relation = self.make_variable()?;

        self.consume(TokenKind::Set, "Expect set for update expression")?;

        let update = Box::new(self.expression()?);

        let mut condition = None;
        if self.matches(TokenKind::Where)? {
            condition = Some(Box::new(self.expression()?));
        }

        Ok(Update {
            relation,
            update,
            condition,
        })
    }

    fn insert(&mut self) -> Res<Expression> {
        self.consume(TokenKind::Into, "Expect into after insert")?;

        self.consume(TokenKind::Identifier, "Expect relation after insert into")?;
        let relation = self.make_variable()?;

        self.consume(
            TokenKind::LeftParen,
            "Expect column declaration after relation in insert into",
        )?;

        let mut columns = vec![];
        while self.matches(TokenKind::Identifier)? {
            columns.push(self.make_variable()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        self.consume(
            TokenKind::RightParen,
            "Expect ) closing columns declaration",
        )?;
        self.consume(
            TokenKind::Values,
            "Expect values after relation declaration",
        )?;

        let mut values = vec![];
        while self.matches(TokenKind::LeftParen)? {
            values.push(self.tuple()?);

            if !self.matches(TokenKind::Comma)? {
                break;
            }
        }

        Ok(Insert {
            relation,
            columns,
            values,
        })
    }

    fn make_variable(&mut self) -> Res<Variable> {
        Ok(Variable {
            name: self.previous.lexeme.clone(),
        })
    }

    fn skip_newlines(&mut self) -> Unit {
        while self.current.kind == TokenKind::Newline {
            self.advance()?;
        }

        Ok(())
    }
}

fn formatting(expr: &Expression) -> String {
    match expr {
        Expression::Select {
            columns,
            from,
            condition,
        } => {
            let mut res = "select ".to_string();

            let mut iter = columns.iter().peekable();
            while let Some(col) = iter.next() {
                res.push_str(&col.name);
                if iter.peek().is_some() {
                    res.push_str(", ");
                }
            }

            res.push_str(&format!(" from {}", from.name));

            if let Some(cond) = condition {
                res.push_str(&format!(" where {}", formatting(cond)))
            }

            res
        }
        Update {
            relation,
            update,
            condition,
        } => {
            let mut res = format!("update {} set {}", relation.name, formatting(update));

            if let Some(cond) = condition {
                res.push_str(&format!(" where {}", formatting(cond)))
            }

            res
        }
        Insert {
            relation,
            columns,
            values,
        } => {
            let mut res = format!("insert {} (", relation.name);

            let mut iter = columns.iter().peekable();
            while let Some(col) = iter.next() {
                res.push_str(&col.name);
                if iter.peek().is_some() {
                    res.push_str(", ");
                }
            }

            res.push_str(") values ");

            let mut iter = values.iter().peekable();
            while let Some(value) = iter.next() {
                res.push_str(&formatting(value));
                if iter.peek().is_some() {
                    res.push_str(", ");
                }
            }

            res
        }
        Expression::Binary {
            left,
            operator,
            right,
        } => {
            let op = match operator {
                Operator::Add => "+",
                Operator::Multiply => "*",
                Operator::Rem => "%",
                Operator::Equal => "=",
                Operator::Is => "is",
                Operator::LessEqual => "<=",
                Operator::Less => "<",
                Operator::Included => "in",
                Operator::And => "and",
            };
            format!("{} {} {}", formatting(left), op, formatting(right))
        }
        Expression::Assignment(var, value) => {
            format!("{} := {}", var.name, formatting(value))
        }
        Expression::Var(var) => var.name.to_string(),
        Expression::Integer(i) => i.to_string(),
        Expression::Set(values) => {
            let mut res = "{".to_string();

            let mut iter = values.iter().peekable();
            while let Some(value) = iter.next() {
                res.push_str(&formatting(value));
                if iter.peek().is_some() {
                    res.push_str(", ");
                }
            }

            res.push('}');

            res
        }
        Expression::Tuple(values) => {
            let mut res = "(".to_string();

            let mut iter = values.iter().peekable();
            while let Some(value) = iter.next() {
                res.push_str(&formatting(value));
                if iter.peek().is_some() {
                    res.push_str(", ");
                }
            }

            res.push(')');

            res
        }
        Expression::Value(v) => v.to_string(),
    }
}

pub fn format_statement(stmt: &Statement) -> String {
    match stmt {
        Statement::Begin(level, Some(tx_name)) => format!("begin {level:?} ({})", tx_name.name),
        Statement::Begin(level, None) => format!("begin {level:?}"),
        Statement::Commit => "commit".to_string(),
        Statement::Abort => "abort".to_string(),
        Statement::Expression(expr) => formatting(expr),
        Statement::Latch => "latch".to_string(),
        Statement::Always(expr) => format!("always[{}]", formatting(expr)),
        Statement::Never(expr) => format!("never[{}]", formatting(expr)),
        Statement::Eventually(expr) => format!("eventually<{}>", formatting(expr)),
    }
}

#[cfg(test)]
mod test {
    use crate::parser::{Expression, Operator, Parser, Statement, Variable};

    #[test]
    fn parse_select_is_value() {
        let mut parser =
            Parser::new("eventually<select age from users where id = 1 is 11>\n".to_string());
        parser.advance().unwrap();

        let mut statements = vec![];
        parser.statement(&mut statements).unwrap();
        assert_eq!(
            Statement::Eventually(Expression::Binary {
                left: Box::new(Expression::Select {
                    columns: vec![Variable {
                        name: "age".to_string()
                    }],
                    from: Variable {
                        name: "users".to_string()
                    },
                    condition: Some(Box::new(Expression::Binary {
                        left: Box::new(Expression::Var(Variable {
                            name: "id".to_string()
                        })),
                        operator: Operator::Equal,
                        right: Box::new(Expression::Integer(1)),
                    })),
                }),
                operator: Operator::Is,
                right: Box::new(Expression::Integer(11)),
            }),
            statements[0]
        );
    }

    #[test]
    fn parse_select_in_value() {
        let mut parser =
            Parser::new("eventually<select age from users where id = 1 in {11}>\n".to_string());
        parser.advance().unwrap();

        let mut statements = vec![];
        parser.statement(&mut statements).unwrap();
        assert_eq!(
            Statement::Eventually(Expression::Binary {
                left: Box::new(Expression::Select {
                    columns: vec![Variable {
                        name: "age".to_string()
                    }],
                    from: Variable {
                        name: "users".to_string()
                    },
                    condition: Some(Box::new(Expression::Binary {
                        left: Box::new(Expression::Var(Variable {
                            name: "id".to_string()
                        })),
                        operator: Operator::Equal,
                        right: Box::new(Expression::Integer(1)),
                    })),
                }),
                operator: Operator::Included,
                right: Box::new(Expression::Set(vec![Expression::Integer(11)])),
            }),
            statements[0]
        );
    }
}

use crate::parser::ParserError::{Unexpected, Uninitialized};
use crate::scanner::{Scanner, ScannerError, Token, TokenKind};
use std::mem;

#[derive(PartialEq, Debug)]
enum Statement {
    // declarations
    Init(Vec<Statement>),
    Process(Vec<Statement>),
    Property(Box<Statement>),
    Assignment(Variable, Expression),

    Begin(IsolationLevel),
    Commit,
    Abort,
    Expression(Expression),
    Latch,

    Always(Expression),
    Never(Expression),
    Eventually(Expression),
}

#[derive(PartialEq, Debug)]
enum IsolationLevel {
    ReadCommitted,
}

#[derive(PartialEq, Debug, Clone)]
struct Variable {
    name: String,
    token: Token,
}

#[derive(PartialEq, Debug, Clone)]
enum Expression {
    Count(Box<Expression>),
    Select {
        columns: Vec<Variable>,
        from: Variable,
        condition: Option<Box<Expression>>,
    },
    Binary {
        left: Box<Expression>,
        operator: Operator,
        right: Box<Expression>,
    },
    Var(Variable),
    Literal(Literal),
}

#[derive(PartialEq, Debug, Clone)]
enum Literal {
    Integer(i16),
    Set(Vec<Expression>),
    Tuple(Vec<Expression>),
}

#[derive(PartialEq, Debug, Clone)]
enum Operator {
    Add,
    Multiply,
    Rem,
    Equal,
    Less,
    Included,
    And,
}

struct Parser {
    scanner: Scanner,
    previous: Result<Token, ParserError>,
    current: Result<Token, ParserError>,
    result: Result<Mets, ParserError>,
}

enum ParserError {
    Scanner(ScannerError),
    Uninitialized,
    Unexpected(String),
}

impl From<ScannerError> for ParserError {
    fn from(value: ScannerError) -> Self {
        ParserError::Scanner(value)
    }
}

struct Mets {
    init: Vec<Statement>,
    processes: Vec<Vec<Statement>>,
    properties: Vec<Statement>,
}

type Res<T> = Result<T, ParserError>;
type Unit = Res<()>;

impl Parser {
    pub fn new(source: String) -> Self {
        Parser {
            scanner: Scanner::new(source),
            previous: Err(Uninitialized),
            current: Err(Uninitialized),
            result: Err(Uninitialized),
        }
    }

    pub fn compile(mut self) -> Res<Mets> {
        self.result = Ok(Mets {
            init: vec![],
            processes: vec![],
            properties: vec![],
        });

        self.advance()?;
        self.skip_newlines()?;
        while !self.matches(TokenKind::Eof)? {
            self.declaration()?;
        }
        self.consume(TokenKind::Eof, "Expect end of expression")?;

        self.result
    }

    fn advance(&mut self) -> Unit {
        mem::swap(&mut self.previous, &mut self.current);
        self.current = Ok(self.scanner.scan_token()?);

        Ok(())
    }

    fn matches(&mut self, kind: TokenKind) -> Res<bool> {
        Ok(if self.current?.kind == kind {
            self.advance()?;
            true
        } else {
            false
        })
    }

    fn consume(&mut self, kind: TokenKind, expected: &str) -> Unit {
        if self.current?.kind == kind {
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
                self.current?.kind
            )))
        }
    }

    fn init_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expect do after init declaration")?;
        self.consume(TokenKind::Newline, "Expect newline after init declaration")?;

        let mut statements = vec![];
        while self.current?.kind != TokenKind::End {
            statements.push(self.statement()?);
        }
        self.result?.init = statements;

        self.consume(TokenKind::End, "Expect end at the end of init declaration")?;

        self.skip_newlines()
    }

    fn process_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Do, "Expect do after process declaration")?;
        self.consume(
            TokenKind::Newline,
            "Expect newline after process declaration",
        )?;

        let mut statements = vec![];
        while self.current?.kind != TokenKind::End {
            statements.push(self.statement()?);
        }
        self.result?.processes.push(statements);

        self.consume(
            TokenKind::End,
            "Expect end at the end of process declaration",
        )?;

        self.skip_newlines()
    }

    fn property_declaration(&mut self) -> Unit {
        self.consume(TokenKind::Equal, "Expect = after property declaration")?;

        self.result?.properties.push(self.statement()?);

        self.consume(
            TokenKind::End,
            "Expect end at the end of property declaration",
        )?;

        self.skip_newlines()
    }

    fn statement(&mut self) -> Res<Statement> {
        let res = if self.matches(TokenKind::Let)? {
            self.assignment_statement()
        } else if self.matches(TokenKind::Begin)? {
            self.begin_statement()
        } else if self.matches(TokenKind::Commit)? {
            self.commit_statement()
        } else if self.matches(TokenKind::Abort)? {
            self.abort_statement()
        } else if self.matches(TokenKind::Latch)? {
            self.latch_statement()
        } else if self.matches(TokenKind::Always)? {
            self.always_statement()
        } else if self.matches(TokenKind::Never)? {
            self.never_statement()
        } else if self.matches(TokenKind::Eventually)? {
            self.eventually_statement()
        } else {
            self.expression_statement()
        };

        self.skip_newlines()?;

        res
    }

    fn assignment_statement(&mut self) -> Res<Statement> {
        let var = self.parse_variable("Expect variable name")?;
        self.consume(TokenKind::ColonEqual, "Expect := after variable name")?;

        let expr = self.expression()?;

        Ok(Statement::Assignment(var, expr))
    }

    fn parse_variable(&mut self, expected: &str) -> Res<Variable> {
        self.consume(TokenKind::Identifier, expected)?;

        Ok(Variable {
            name: self.previous?.lexeme,
            token: self.previous?.clone(),
        })
    }

    fn begin_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::Identifier, "Expect isolation level after begin")?;

        match self.previous?.lexeme.as_str() {
            "read_committed" => Ok(Statement::Begin(IsolationLevel::ReadCommitted)),
            _ => Err(Unexpected("Expected following isolation level: read_committed".to_string()))
        }
    }

    fn commit_statement(&mut self) -> Res<Statement> {
        Ok(Statement::Commit)
    }

    fn abort_statement(&mut self) -> Res<Statement> {
        Ok(Statement::Abort)
    }

    fn latch_statement(&mut self) -> Res<Statement> {
        Ok(Statement::Latch)
    }

    fn always_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::LeftBracket, "Expect [ to open always statement")?;

        let expr = self.expression()?;

        self.consume(TokenKind::RightBracket, "Expect ] to close always statement")?;
        Ok(Statement::Always(expr))
    }

    fn never_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::LeftBracket, "Expect [ to open never statement")?;

        let expr = self.expression()?;

        self.consume(TokenKind::RightBracket, "Expect ] to close never statement")?;
        Ok(Statement::Never(expr))
    }

    fn eventually_statement(&mut self) -> Res<Statement> {
        self.consume(TokenKind::LeftCarret, "Expect < to open eventually statement")?;

        let expr = self.expression()?;

        self.consume(TokenKind::RightCarret, "Expect > to close eventually statement")?;
        Ok(Statement::Eventually(expr))
    }

    fn expression_statement(&mut self) -> Res<Statement> {
        let expr = self.expression()?;
        Ok(Statement::Expression(expr))
    }

    fn expression(&mut self) -> Res<Expression> {

    }

    fn skip_newlines(&mut self) -> Unit {
        while self.current?.kind == TokenKind::Newline {
            self.advance()?;
        }

        Ok(())
    }
}

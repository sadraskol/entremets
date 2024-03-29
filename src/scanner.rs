#[derive(PartialEq, Debug, Clone)]
pub struct Position {
    pub start_line: usize,
    pub start_col: usize,

    pub end_line: usize,
    pub end_col: usize,
}

impl Position {
    pub fn new() -> Self {
        Position {
            start_line: 1,
            start_col: 1,
            end_line: 1,
            end_col: 1,
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub position: Position,
}

impl Token {
    pub fn uninitialized() -> Self {
        Token {
            kind: TokenKind::Error,
            lexeme: "".to_string(),
            position: Position::new(),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TokenKind {
    Newline,
    ColonEqual,
    Comma,
    Dot,
    Star,
    Plus,
    Minus,
    Percent,
    Slash,
    Equal,
    Different,
    LeftArrow,
    Backtick,
    Dollar,
    LessEqual,
    GreaterEqual,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    LeftCarret,
    RightCarret,
    If,
    Else,
    Do,
    End,
    Transaction,
    Begin,
    Commit,
    Abort,
    Count,
    Create,
    Unique,
    Index,
    On,
    Select,
    From,
    Where,
    Order,
    By,
    Limit,
    Offset,
    Insert,
    Delete,
    Into,
    Values,
    Update,
    For,
    Set,
    Between,
    Alter,
    Table,
    Add,
    Constraint,
    Foreign,
    Key,
    References,
    In,
    And,
    Or,
    Always,
    Never,
    Eventually,
    Property,
    Process,
    Latch,
    Init,
    Let,
    Identifier,
    Number,
    String,
    Eof,
    Error,
}

#[derive(Copy, Clone)]
struct Cursor {
    index: usize,
    col: usize,
    line: usize,
}

impl Cursor {
    fn new() -> Self {
        Cursor {
            index: 0,
            line: 1,
            col: 1,
        }
    }

    fn advance(&mut self) {
        self.index += 1;
        self.col += 1;
    }

    fn newline(&mut self) {
        self.index += 1;
        self.col = 1;
        self.line += 1;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScannerError {
    expected: String,
    lexeme: String,
    position: Position,
}

#[derive(Clone)]
pub struct Scanner {
    source: String,
    start: Cursor,
    current: Cursor,
}

impl Scanner {
    pub fn new(source: String) -> Scanner {
        Scanner {
            source,
            start: Cursor::new(),
            current: Cursor::new(),
        }
    }

    pub fn scan_token(&mut self) -> Result<Token, ScannerError> {
        self.skip_whitespace();
        self.start = self.current;
        if self.is_at_end() {
            self.make_token(TokenKind::Eof)
        } else {
            let c = if self.peek() == '\n' {
                self.newline()
            } else {
                self.advance()
            };

            if c.is_alphabetic() || c == '_' {
                return self.identifier();
            }
            if c.is_numeric() {
                return self.number();
            }

            match c {
                '\n' => self.make_token(TokenKind::Newline),
                '`' => self.make_token(TokenKind::Backtick),
                '$' => self.make_token(TokenKind::Dollar),
                '(' => self.make_token(TokenKind::LeftParen),
                ')' => self.make_token(TokenKind::RightParen),
                '[' => self.make_token(TokenKind::LeftBracket),
                ']' => self.make_token(TokenKind::RightBracket),
                '{' => self.make_token(TokenKind::LeftBrace),
                '}' => self.make_token(TokenKind::RightBrace),
                ',' => self.make_token(TokenKind::Comma),
                '+' => self.make_token(TokenKind::Plus),
                '-' => self.make_token(TokenKind::Minus),
                '/' => self.make_token(TokenKind::Slash),
                '%' => self.make_token(TokenKind::Percent),
                '*' => self.make_token(TokenKind::Star),
                '.' => self.make_token(TokenKind::Dot),
                ':' => {
                    if self.matches('=') {
                        self.make_token(TokenKind::ColonEqual)
                    } else {
                        self.make_error("Expected =")
                    }
                }
                '>' => {
                    if self.matches('=') {
                        self.make_token(TokenKind::GreaterEqual)
                    } else {
                        self.make_token(TokenKind::RightCarret)
                    }
                }
                '<' => {
                    if self.matches('-') {
                        self.make_token(TokenKind::LeftArrow)
                    } else if self.matches('=') {
                        self.make_token(TokenKind::LessEqual)
                    } else if self.matches('>') {
                        self.make_token(TokenKind::Different)
                    } else {
                        self.make_token(TokenKind::LeftCarret)
                    }
                }
                '=' => self.make_token(TokenKind::Equal),
                '\'' => self.string(),
                _ => self.make_error("Expected valid token"),
            }
        }
    }

    fn identifier(&mut self) -> Result<Token, ScannerError> {
        while !self.is_at_end()
            && (self.peek().is_alphabetic() || self.peek() == '_' || self.peek().is_numeric())
        {
            self.advance();
        }
        self.make_token(self.identifier_type())
    }

    fn identifier_type(&self) -> TokenKind {
        match self.source.chars().nth(self.start.index).unwrap() {
            'a' => {
                if self.current.index - self.start.index > 2 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'b' => self.check_keyword(2, "ort", TokenKind::Abort),
                        'd' => self.check_keyword(2, "d", TokenKind::Add),
                        'n' => self.check_keyword(2, "d", TokenKind::And),
                        'l' => match self.source.chars().nth(self.start.index + 2).unwrap() {
                            'w' => self.check_keyword(3, "ays", TokenKind::Always),
                            't' => self.check_keyword(3, "er", TokenKind::Alter),
                            _ => TokenKind::Identifier,
                        },
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'b' => {
                if self.current.index - self.start.index > 3
                    && self.source.chars().nth(self.start.index + 1).unwrap() == 'e'
                {
                    match self.source.chars().nth(self.start.index + 2).unwrap() {
                        'g' => self.check_keyword(3, "gin", TokenKind::Begin),
                        't' => self.check_keyword(3, "ween", TokenKind::Between),
                        _ => TokenKind::Identifier,
                    }
                } else if self.current.index - self.start.index == 2
                    && self.source.chars().nth(self.start.index + 1).unwrap() == 'y'
                {
                    TokenKind::By
                } else {
                    TokenKind::Identifier
                }
            }
            'c' => {
                if self.current.index - self.start.index > 2 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'o' => match self.source.chars().nth(self.start.index + 2).unwrap() {
                            'm' => self.check_keyword(3, "mit", TokenKind::Commit),
                            'n' => self.check_keyword(3, "straint", TokenKind::Constraint),
                            'u' => self.check_keyword(3, "nt", TokenKind::Count),
                            _ => TokenKind::Identifier,
                        },
                        'r' => self.check_keyword(2, "eate", TokenKind::Create),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'd' => {
                if self.current.index - self.start.index > 1 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'o' => self.check_keyword(2, "", TokenKind::Do),
                        'e' => self.check_keyword(2, "lete", TokenKind::Delete),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'e' => {
                if self.current.index - self.start.index > 2 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'l' => self.check_keyword(2, "se", TokenKind::Else),
                        'n' => self.check_keyword(2, "d", TokenKind::End),
                        'v' => self.check_keyword(2, "entually", TokenKind::Eventually),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'f' => {
                if self.current.index - self.start.index > 2 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'o' if self.current.index - self.start.index == 3 => {
                            self.check_keyword(2, "r", TokenKind::For)
                        }
                        'o' if self.current.index - self.start.index > 3 => {
                            self.check_keyword(2, "reign", TokenKind::Foreign)
                        }
                        'r' => self.check_keyword(2, "om", TokenKind::From),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'i' => match self.current.index - self.start.index {
                2 => match self.source.chars().nth(self.start.index + 1).unwrap() {
                    'n' => self.check_keyword(2, "", TokenKind::In),
                    'f' => self.check_keyword(2, "", TokenKind::If),
                    _ => TokenKind::Identifier,
                },
                x if x > 2 => match self.source.chars().nth(self.start.index + 1).unwrap() {
                    'n' => match self.source.chars().nth(self.start.index + 2).unwrap() {
                        'd' => self.check_keyword(3, "ex", TokenKind::Index),
                        'i' => self.check_keyword(3, "t", TokenKind::Init),
                        's' => self.check_keyword(3, "ert", TokenKind::Insert),
                        't' => self.check_keyword(3, "o", TokenKind::Into),
                        _ => TokenKind::Identifier,
                    },
                    _ => TokenKind::Identifier,
                },
                _ => TokenKind::Identifier,
            },
            'k' => self.check_keyword(1, "ey", TokenKind::Key),
            'n' => self.check_keyword(1, "ever", TokenKind::Never),
            'l' => {
                if self.current.index - self.start.index > 1 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'a' => self.check_keyword(2, "tch", TokenKind::Latch),
                        'e' => self.check_keyword(2, "t", TokenKind::Let),
                        'i' => self.check_keyword(2, "mit", TokenKind::Limit),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'o' => {
                if self.current.index - self.start.index > 1 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'f' => self.check_keyword(2, "fset", TokenKind::Offset),
                        'r' if self.current.index - self.start.index == 2 => TokenKind::Or,
                        'r' => self.check_keyword(2, "der", TokenKind::Order),
                        'n' if self.current.index - self.start.index == 2 => TokenKind::On,
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'p' => {
                if self.current.index - self.start.index > 6 {
                    if self
                        .source
                        .chars()
                        .skip(self.start.index)
                        .take(3)
                        .collect::<String>()
                        == *"pro".to_string()
                    {
                        match self.source.chars().nth(self.start.index + 3).unwrap() {
                            'c' => self.check_keyword(4, "ess", TokenKind::Process),
                            'p' => self.check_keyword(4, "erty", TokenKind::Property),
                            _ => TokenKind::Identifier,
                        }
                    } else {
                        TokenKind::Identifier
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'r' => self.check_keyword(1, "eferences", TokenKind::References),
            's' => {
                if self
                    .source
                    .chars()
                    .skip(self.start.index)
                    .take(2)
                    .collect::<String>()
                    == *"se".to_string()
                {
                    match self.source.chars().nth(self.start.index + 2).unwrap() {
                        'l' => self.check_keyword(3, "ect", TokenKind::Select),
                        't' => self.check_keyword(3, "", TokenKind::Set),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            't' => {
                if self.current.index - self.start.index > 3 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'a' => self.check_keyword(2, "ble", TokenKind::Table),
                        'r' => self.check_keyword(2, "ansaction", TokenKind::Transaction),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'u' => {
                if self.current.index - self.start.index > 5 {
                    match self.source.chars().nth(self.start.index + 1).unwrap() {
                        'p' => self.check_keyword(2, "date", TokenKind::Update),
                        'n' => self.check_keyword(2, "ique", TokenKind::Unique),
                        _ => TokenKind::Identifier,
                    }
                } else {
                    TokenKind::Identifier
                }
            }
            'v' => self.check_keyword(1, "alues", TokenKind::Values),
            'w' => self.check_keyword(1, "here", TokenKind::Where),
            _ => TokenKind::Identifier,
        }
    }

    fn number(&mut self) -> Result<Token, ScannerError> {
        while self.peek().is_numeric() {
            self.advance();
        }

        self.make_token(TokenKind::Number)
    }

    fn string(&mut self) -> Result<Token, ScannerError> {
        while self.peek() != '\'' {
            self.advance();
        }

        self.advance(); // consume closing '

        self.make_token(TokenKind::String)
    }

    fn skip_whitespace(&mut self) {
        loop {
            if self.is_at_end() {
                break;
            }
            let c = self.peek();
            if c.is_whitespace() {
                if c == '\n' {
                    break;
                } else {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn check_keyword(&self, start: usize, rest: &str, kind: TokenKind) -> TokenKind {
        let length = rest.len();
        if self.current.index - self.start.index == start + length
            && rest
                == self
                    .source
                    .chars()
                    .skip(self.start.index + start)
                    .take(length)
                    .collect::<String>()
        {
            kind
        } else {
            TokenKind::Identifier
        }
    }

    fn peek(&self) -> char {
        self.source.chars().nth(self.current.index).unwrap()
    }

    fn matches(&mut self, c: char) -> bool {
        if self.source.chars().nth(self.current.index) == Some(c) {
            self.current.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) -> char {
        self.current.advance();
        self.source.chars().nth(self.current.index - 1).unwrap()
    }

    fn newline(&mut self) -> char {
        self.current.newline();
        self.source.chars().nth(self.current.index - 1).unwrap()
    }

    fn is_at_end(&self) -> bool {
        self.current.index == self.source.chars().count()
    }

    fn make_token(&self, kind: TokenKind) -> Result<Token, ScannerError> {
        Ok(Token {
            kind,
            lexeme: self
                .source
                .chars()
                .skip(self.start.index)
                .take(self.current.index - self.start.index)
                .collect::<String>(),
            position: Position {
                start_line: self.start.line,
                start_col: self.start.col,
                end_line: self.current.line,
                end_col: self.current.col,
            },
        })
    }

    fn make_error(&self, expected_message: &str) -> Result<Token, ScannerError> {
        Err(ScannerError {
            expected: expected_message.to_string(),
            lexeme: self
                .source
                .chars()
                .skip(self.start.index)
                .take(self.current.index - self.start.index)
                .collect::<String>(),
            position: Position {
                start_line: self.start.line,
                start_col: self.start.col,
                end_line: self.current.line,
                end_col: self.current.col,
            },
        })
    }
}

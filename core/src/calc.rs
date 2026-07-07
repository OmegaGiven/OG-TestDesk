/// Recursive-descent arithmetic expression evaluator supporting
/// `+ - * / % ()` and unary minus, over f64. No external logic to port from —
/// the original app's calculator was entirely client-side JavaScript.
pub fn evaluate(expr: &str) -> Result<f64, String> {
    let tokens = tokenize(expr)?;
    if tokens.is_empty() {
        return Err("Empty expression".to_string());
    }
    let mut parser = Parser { tokens, pos: 0 };
    let value = parser.parse_expr()?;
    if parser.pos != parser.tokens.len() {
        return Err("Unexpected trailing input".to_string());
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
}

fn tokenize(expr: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            ' ' | '\t' | '\n' => i += 1,
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let value = text
                    .parse::<f64>()
                    .map_err(|_| format!("Invalid number: {text}"))?;
                tokens.push(Token::Number(value));
            }
            other => return Err(format!("Unexpected character: {other}")),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        token
    }

    // expr := term (('+' | '-') term)*
    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut value = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.next();
                    value += self.parse_term()?;
                }
                Some(Token::Minus) => {
                    self.next();
                    value -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    // term := factor (('*' | '/' | '%') factor)*
    fn parse_term(&mut self) -> Result<f64, String> {
        let mut value = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.next();
                    value *= self.parse_unary()?;
                }
                Some(Token::Slash) => {
                    self.next();
                    let divisor = self.parse_unary()?;
                    if divisor == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    value /= divisor;
                }
                Some(Token::Percent) => {
                    self.next();
                    let divisor = self.parse_unary()?;
                    if divisor == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    value %= divisor;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    // unary := '-' unary | primary
    fn parse_unary(&mut self) -> Result<f64, String> {
        if let Some(Token::Minus) = self.peek() {
            self.next();
            return Ok(-self.parse_unary()?);
        }
        if let Some(Token::Plus) = self.peek() {
            self.next();
            return self.parse_unary();
        }
        self.parse_primary()
    }

    // primary := NUMBER | '(' expr ')'
    fn parse_primary(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Token::Number(value)) => Ok(value),
            Some(Token::LParen) => {
                let value = self.parse_expr()?;
                match self.next() {
                    Some(Token::RParen) => Ok(value),
                    _ => Err("Expected closing parenthesis".to_string()),
                }
            }
            other => Err(format!("Unexpected token: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn basic_arithmetic() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(evaluate("(2 + 3) * 4").unwrap(), 20.0);
        assert_eq!(evaluate("-5 + 2").unwrap(), -3.0);
        assert_eq!(evaluate("10 % 3").unwrap(), 1.0);
        assert!(evaluate("1 / 0").is_err());
    }
}

//! Palette calculator: `=2*(3+4)` → 14. Supports + - * / % ^, parentheses, unary minus and
//! the constant `pi`.

/// Evaluate an expression; `None` when it is incomplete or invalid.
pub fn eval(expr: &str) -> Option<f64> {
    let tokens: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser { t: &tokens, i: 0 };
    let v = p.sum()?;
    (p.i == tokens.len() && v.is_finite()).then_some(v)
}

/// Result text: integers without a fraction, others to 10 significant places.
pub fn format(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

struct Parser<'a> {
    t: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.t.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let hit = self.peek() == Some(c);
        self.i += hit as usize;
        hit
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        loop {
            if self.eat('+') {
                v += self.product()?;
            } else if self.eat('-') {
                v -= self.product()?;
            } else {
                return Some(v);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') {
                v *= self.unary()?;
            } else if self.eat('/') {
                v /= self.unary()?;
            } else if self.eat('%') {
                v %= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }

    /// Minus binds looser than `^`: -2^2 = -4.
    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    /// Right-associative: 2^3^2 = 2^9; the exponent may be negative (2^-1).
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat('^') {
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        if self.eat('(') {
            let v = self.sum()?;
            return self.eat(')').then_some(v);
        }
        let rest: String = self.t[self.i..].iter().take(2).collect();
        if rest.eq_ignore_ascii_case("pi") {
            self.i += 2;
            return Some(std::f64::consts::PI);
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.i += 1;
        }
        let text: String = self.t[start..self.i].iter().collect();
        text.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(eval("2*3"), Some(6.0));
        assert_eq!(eval("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("-2^2"), Some(-4.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("10 % 4"), Some(2.0));
        assert_eq!(eval("1.5*2"), Some(3.0));
        assert!((eval("pi").unwrap() - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn invalid_input() {
        assert_eq!(eval(""), None);
        assert_eq!(eval("2*"), None);
        assert_eq!(eval("(1"), None);
        assert_eq!(eval("1/0"), None);
        assert_eq!(eval("abc"), None);
    }

    #[test]
    fn formatting() {
        assert_eq!(format(6.0), "6");
        assert_eq!(format(0.5), "0.5");
        assert_eq!(format(1.0 / 3.0), "0.3333333333");
    }
}

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Calc;
impl Handler for Calc {
    fn spec(&self) -> Tool {
        Tool::function("calc", "Evaluate an arithmetic expression exactly. Use for ANY math so the number is right. Supports + - * / % ^, parentheses, unary minus, constants pi and e, and functions sqrt, abs, floor, ceil, round, log (ln), log10, exp, sin, cos, tan, min(a,b,...), max(a,b,...), pow(a,b).", json!({
            "type": "object",
            "properties": { "expr": { "type": "string", "description": "Arithmetic expression to evaluate" } },
            "required": ["expr"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let expr = args["expr"].as_str().ok_or_else(|| anyhow::anyhow!("missing expr"))?;
        match parse(expr) {
            Ok(v) => Ok(fmt_num(v)),
            Err(e) => anyhow::bail!("calc error: {e}"),
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

fn parse(input: &str) -> anyhow::Result<f64> {
    let mut p = Parser { s: input.as_bytes(), i: 0 };
    let v = p.expr()?;
    p.ws();
    if p.i < p.s.len() {
        anyhow::bail!("unexpected '{}' at pos {}", p.s[p.i] as char, p.i);
    }
    Ok(v)
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.i < self.s.len() && self.s[self.i] == c {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> anyhow::Result<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn term(&mut self) -> anyhow::Result<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else if self.eat(b'%') {
                v %= self.unary()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> anyhow::Result<f64> {
        if self.eat(b'-') {
            Ok(-self.unary()?)
        } else if self.eat(b'+') {
            self.unary()
        } else {
            self.power_chain()
        }
    }

    fn power_chain(&mut self) -> anyhow::Result<f64> {
        let mut v = self.primary()?;
        if self.eat(b'^') {
            v = v.powf(self.unary()?);
        }
        Ok(v)
    }

    fn primary(&mut self) -> anyhow::Result<f64> {
        self.ws();
        if self.i >= self.s.len() {
            anyhow::bail!("unexpected end of expression");
        }
        let c = self.s[self.i];
        if c.is_ascii_digit() || c == b'.' {
            return self.number();
        }
        if c == b'(' {
            self.i += 1;
            let v = self.expr()?;
            if !self.eat(b')') {
                anyhow::bail!("missing ')'");
            }
            return Ok(v);
        }
        if c.is_ascii_alphabetic() {
            return self.ident();
        }
        anyhow::bail!("unexpected '{}'", c as char)
    }

    fn number(&mut self) -> anyhow::Result<f64> {
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.')
        {
            self.i += 1;
        }
        if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
            let save = self.i;
            self.i += 1;
            if self.i < self.s.len() && (self.s[self.i] == b'+' || self.s[self.i] == b'-') {
                self.i += 1;
            }
            let exp_start = self.i;
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
            }
            if self.i == exp_start {
                self.i = save;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i])
            .map_err(|_| anyhow::anyhow!("bad number"))?;
        text.parse::<f64>()
            .map_err(|_| anyhow::anyhow!("bad number '{text}'"))
    }

    fn ident(&mut self) -> anyhow::Result<f64> {
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i])?.to_string();
        if !self.eat(b'(') {
            return match name.as_str() {
                "pi" => Ok(std::f64::consts::PI),
                "e" => Ok(std::f64::consts::E),
                _ => anyhow::bail!("unknown constant '{name}'"),
            };
        }
        let mut args = Vec::new();
        loop {
            args.push(self.expr()?);
            if self.eat(b',') {
                continue;
            }
            if self.eat(b')') {
                break;
            }
            anyhow::bail!("expected ',' or ')' in {name}(...)");
        }
        Ok(match name.as_str() {
            "sqrt" => one(&args, &name)?.sqrt(),
            "abs" => one(&args, &name)?.abs(),
            "floor" => one(&args, &name)?.floor(),
            "ceil" => one(&args, &name)?.ceil(),
            "round" => one(&args, &name)?.round(),
            "log" => one(&args, &name)?.ln(),
            "log10" => one(&args, &name)?.log10(),
            "exp" => one(&args, &name)?.exp(),
            "sin" => one(&args, &name)?.sin(),
            "cos" => one(&args, &name)?.cos(),
            "tan" => one(&args, &name)?.tan(),
            "pow" => {
                if args.len() != 2 {
                    anyhow::bail!("pow needs 2 args");
                }
                args[0].powf(args[1])
            }
            "min" => args.iter().copied().fold(f64::INFINITY, f64::min),
            "max" => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            _ => anyhow::bail!("unknown function '{name}'"),
        })
    }
}

fn one(args: &[f64], name: &str) -> anyhow::Result<f64> {
    if args.len() != 1 {
        anyhow::bail!("{name} needs 1 arg");
    }
    Ok(args[0])
}

fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "NaN".to_string()
        } else if v > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{:.0}", v);
    }
    let s = format!("{:.15}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(expr: &str) -> String {
        Calc.run(&json!({ "expr": expr })).unwrap()
    }

    #[test]
    fn eval_arithmetic() {
        assert_eq!(run("2 + 3 * 4"), "14");
        assert_eq!(run("(2 + 3) * 4"), "20");
        assert_eq!(run("-2^2"), "-4");
        assert_eq!(run("2^-2"), "0.25");
        assert_eq!(run("10 % 3"), "1");
        assert_eq!(run("1.5e3"), "1500");
    }

    #[test]
    fn eval_functions() {
        assert_eq!(run("sqrt(16)"), "4");
        assert_eq!(run("max(3, 7, 2)"), "7");
        assert_eq!(run("min(3, 7, 2)"), "2");
        assert_eq!(run("pow(2, 10)"), "1024");
        assert_eq!(run("floor(pi)"), "3");
        assert_eq!(run("round(2.5)"), "3");
    }

    #[test]
    fn eval_floats_trailing_zeros_trimmed() {
        assert_eq!(run("0.1 + 0.2"), "0.3");
        assert_eq!(run("7 / 2"), "3.5");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Calc.run(&json!({ "expr": "2 +" })).is_err());
        assert!(Calc.run(&json!({ "expr": "wat(1)" })).is_err());
        assert!(Calc.run(&json!({})).is_err());
    }
}
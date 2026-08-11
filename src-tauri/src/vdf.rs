//! Minimal Valve KeyValues (VDF) parser.
//!
//! Steam's `loginusers.vdf`, `libraryfolders.vdf` and `config.vdf` are all
//! text KeyValues: quoted keys, either a quoted value or a `{ ... }` block.
//! Real files contain tabs, `//` comments, escaped quotes and (in config.vdf)
//! conditional suffixes like `[$WIN32]`, which we skip.
//!
//! Pulling in a crate for ~120 lines of parsing isn't worth the dependency.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Obj(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m
                .get(key)
                .or_else(|| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }
    /// `root.path("users/76561198.../AccountName")`
    pub fn path(&self, p: &str) -> Option<&Value> {
        p.split('/').try_fold(self, |cur, seg| cur.get(seg))
    }
}

pub fn parse(input: &str) -> Result<Value, String> {
    let bytes: Vec<char> = input.chars().collect();
    let mut p = Parser { b: bytes, i: 0 };
    let mut root = BTreeMap::new();
    p.skip_ws();
    while p.i < p.b.len() {
        let (k, v) = p.pair()?;
        root.insert(k, v);
        p.skip_ws();
    }
    Ok(Value::Obj(root))
}

struct Parser {
    b: Vec<char>,
    i: usize,
}

impl Parser {
    fn skip_ws(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_whitespace() {
                self.i += 1;
            }
            // `//` line comment
            if self.i + 1 < self.b.len() && self.b[self.i] == '/' && self.b[self.i + 1] == '/' {
                while self.i < self.b.len() && self.b[self.i] != '\n' {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
    }

    fn token(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.i >= self.b.len() {
            return Err("unexpected end of input".into());
        }
        if self.b[self.i] == '"' {
            self.i += 1;
            let mut out = String::new();
            while self.i < self.b.len() {
                match self.b[self.i] {
                    '\\' if self.i + 1 < self.b.len() => {
                        out.push(match self.b[self.i + 1] {
                            'n' => '\n',
                            't' => '\t',
                            c => c,
                        });
                        self.i += 2;
                    }
                    '"' => {
                        self.i += 1;
                        return Ok(out);
                    }
                    c => {
                        out.push(c);
                        self.i += 1;
                    }
                }
            }
            return Err("unterminated quoted string".into());
        }
        // Bare token (rare, but config.vdf has them).
        let start = self.i;
        while self.i < self.b.len()
            && !self.b[self.i].is_whitespace()
            && self.b[self.i] != '{'
            && self.b[self.i] != '}'
        {
            self.i += 1;
        }
        if start == self.i {
            return Err(format!("empty token at offset {}", self.i));
        }
        Ok(self.b[start..self.i].iter().collect())
    }

    fn pair(&mut self) -> Result<(String, Value), String> {
        let key = self.token()?;
        self.skip_ws();
        // Conditional suffix, e.g. `"key" "value" [$WIN32]` — ignore it.
        if self.i < self.b.len() && self.b[self.i] == '{' {
            self.i += 1;
            let mut map = BTreeMap::new();
            loop {
                self.skip_ws();
                if self.i >= self.b.len() {
                    return Err("unterminated block".into());
                }
                if self.b[self.i] == '}' {
                    self.i += 1;
                    break;
                }
                let (k, v) = self.pair()?;
                map.insert(k, v);
            }
            return Ok((key, Value::Obj(map)));
        }
        let val = self.token()?;
        self.skip_ws();
        if self.i < self.b.len() && self.b[self.i] == '[' {
            while self.i < self.b.len() && self.b[self.i] != ']' {
                self.i += 1;
            }
            self.i = (self.i + 1).min(self.b.len());
        }
        Ok((key, Value::Str(val)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGINUSERS: &str = r#"
"users"
{
	"76561198012345678"
	{
		"AccountName"		"testuser"
		"PersonaName"		"Test User"
		"RememberPassword"		"1"
		"WantsOfflineMode"		"0"
		"MostRecent"		"1"
		"Timestamp"		"1723400000"
	}
}
"#;

    #[test]
    fn parses_loginusers() {
        let v = parse(LOGINUSERS).unwrap();
        assert_eq!(
            v.path("users/76561198012345678/AccountName")
                .and_then(Value::as_str),
            Some("testuser")
        );
        assert_eq!(
            v.path("users/76561198012345678/RememberPassword")
                .and_then(Value::as_str),
            Some("1")
        );
    }

    #[test]
    fn is_case_insensitive_on_keys() {
        let v = parse(LOGINUSERS).unwrap();
        assert!(v.path("Users/76561198012345678/accountname").is_some());
    }

    #[test]
    fn skips_comments_and_conditionals() {
        let src = "// leading comment\n\"a\"\n{\n  \"b\" \"c\" [$WIN32]\n  \"d\" \"e\"\n}\n";
        let v = parse(src).unwrap();
        assert_eq!(v.path("a/b").and_then(Value::as_str), Some("c"));
        assert_eq!(v.path("a/d").and_then(Value::as_str), Some("e"));
    }

    #[test]
    fn rejects_unterminated_blocks() {
        assert!(parse("\"a\"\n{\n \"b\" \"c\"\n").is_err());
    }
}

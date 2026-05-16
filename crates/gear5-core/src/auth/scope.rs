use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Read,
    Admin,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Scope::Read),
            "admin" => Some(Scope::Admin),
            _ => None,
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn has_scope(scopes: &[String], wanted: Scope) -> bool {
    scopes.iter().any(|s| s == wanted.as_str())
}

use std::fmt;
use std::str;

use column::{Column};
use table::{Table};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ForeignKeySpecification {
    pub name: Option<String>,
    pub ref_action: Option<String>,
    pub from: Vec<Column>,
    pub that_table: Table,
    pub to: Vec<Column>,
}

impl fmt::Display for ForeignKeySpecification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref name) = self.name {
            write!(f, "CONSTRAINT {} ", name)?;
        }

        let mut cnt = 0;
        for c in self.from.iter() {
            if cnt == 0 {
                write!(f, "FOREIGN KEY({}", c)?;
            } else {
                write!(f, ",{}", c)?;
            }
            cnt += 1;
        }
        if cnt > 0 {
            write!(f, ")")?;
        }
        write!(f, " REFERENCES {}", self.that_table.clone())?;
        cnt = 0;
        for c in self.to.iter() {
            if cnt == 0 {
                write!(f, "({}", c)?;
            } else {
                write!(f, ",{}", c)?;
            }
            cnt += 1;
        }
        if cnt > 0 {
            write!(f, ")")?;
        }

        if let Some(ref ref_action) = self.ref_action {
            write!(f, " {} ", ref_action)?;
        }

        Ok(())
    }
}

impl ForeignKeySpecification {
    pub fn new(name: Option<String>, ref_action: Option<String>, from: Vec<Column>, that_table: Table, to: Vec<Column>) -> ForeignKeySpecification {
        ForeignKeySpecification {
            name: name,
            ref_action: ref_action,
            from: from,
            that_table: that_table,
            to: to,
        }
    }
}

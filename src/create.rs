use nom::{digit, multispace};
use nom::types::CompleteByteSlice;
use std::fmt;
use std::str;
use std::str::FromStr;

use create_table_options::table_options;
use column::{Column, ColumnConstraint, ColumnSpecification};
use common::{
    column_identifier_no_alias, opt_multispace, parse_comment, sql_identifier,
    statement_terminator, table_reference, type_identifier, Literal, Real, SqlType,
    TableKey,
};
use compound_select::{compound_selection, CompoundSelectStatement};
use keywords::escape_if_keyword;
use order::{order_type, OrderType};
use select::{nested_selection, SelectStatement};
use table::Table;
use foreignkey::{ForeignKeySpecification};

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CreateTableStatement {
    pub table: Table,
    pub fields: Vec<ColumnSpecification>,
    pub keys: Option<Vec<TableKey>>,
    pub fkeys: Option<Vec<ForeignKeySpecification>>,
}

impl fmt::Display for CreateTableStatement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CREATE TABLE {} ", escape_if_keyword(&self.table.name))?;
        write!(f, "(")?;
        write!(
            f,
            "{}",
            self.fields
                .iter()
                .map(|field| format!("{}", field))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        if let Some(ref keys) = self.keys {
            write!(
                f,
                ", {}",
                keys.iter()
                    .map(|key| format!("{}", key))
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        if let Some(ref fkeys) = self.fkeys {
            write!(
                f,
                ", {}",
                fkeys.iter()
                    .map(|fkey| format!("{}", fkey))
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        write!(f, ")")
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum SelectSpecification {
    Compound(CompoundSelectStatement),
    Simple(SelectStatement),
}

impl fmt::Display for SelectSpecification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SelectSpecification::Compound(ref csq) => write!(f, "{}", csq),
            SelectSpecification::Simple(ref sq) => write!(f, "{}", sq),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CreateViewStatement {
    pub name: String,
    pub fields: Vec<Column>,
    pub definition: Box<SelectSpecification>,
}

impl fmt::Display for CreateViewStatement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CREATE VIEW {} ", escape_if_keyword(&self.name))?;
        if !self.fields.is_empty() {
            write!(f, "(")?;
            write!(
                f,
                "{}",
                self.fields
                    .iter()
                    .map(|field| format!("{}", field))
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
            write!(f, ") ")?;
        }
        write!(f, "AS ")?;
        write!(f, "{}", self.definition)
    }
}

/// MySQL grammar element for index column definition (§13.1.18, index_col_name)
named!(pub index_col_name<CompleteByteSlice, (Column, Option<u16>, Option<OrderType>)>,
    do_parse!(
        column: column_identifier_no_alias >>
        opt_multispace >>
        len: opt!(delimited!(tag!("("), digit, tag!(")"))) >>
        order: opt!(order_type) >>
        ((column, len.map(|l| u16::from_str(str::from_utf8(*l).unwrap()).unwrap()), order))
    )
);

/// Helper for list of index columns
named!(pub index_col_list<CompleteByteSlice, Vec<Column> >,
       many0!(
           do_parse!(
               entry: index_col_name >>
               opt!(
                   do_parse!(
                       opt_multispace >>
                       tag!(",") >>
                       opt_multispace >>
                       ()
                   )
               ) >>
               // XXX(malte): ignores length and order
               (entry.0)
           )
       )
);

/// Parse rule for an individual key specification.
named!(pub key_specification<CompleteByteSlice, TableKey>,
    alt!(
          do_parse!(
              tag_no_case!("fulltext") >>
              multispace >>
              alt!(tag_no_case!("key") | tag_no_case!("index")) >>
              opt_multispace >>
              name: opt!(sql_identifier) >>
              opt_multispace >>
              columns: delimited!(tag!("("), delimited!(opt_multispace, index_col_list, opt_multispace), tag!(")")) >>
              (match name {
                  Some(name) => {
                      let n = String::from_utf8(name.to_vec()).unwrap();
                      TableKey::FulltextKey(Some(n), columns)
                  },
                  None => TableKey::FulltextKey(None, columns),
              })
          )
        | do_parse!(
              tag_no_case!("primary key") >>
              opt_multispace >>
              columns: delimited!(tag!("("), delimited!(opt_multispace, index_col_list, opt_multispace), tag!(")")) >>
              opt!(do_parse!(
                          multispace >>
                          tag_no_case!("autoincrement") >>
                          ()
                   )
              ) >>
              (TableKey::PrimaryKey(columns))
          )
        | do_parse!(
              tag_no_case!("unique") >>
              opt!(preceded!(multispace,
                             alt!(
                                   tag_no_case!("key")
                                 | tag_no_case!("index")
                             )
                   )
              ) >>
              opt_multispace >>
              name: opt!(sql_identifier) >>
              opt_multispace >>
              columns: delimited!(tag!("("), delimited!(opt_multispace, index_col_list, opt_multispace), tag!(")")) >>
              (match name {
                  Some(name) => {
                      let n = String::from_utf8(name.to_vec()).unwrap();
                      TableKey::UniqueKey(Some(n), columns)
                  },
                  None => TableKey::UniqueKey(None, columns),
              })
          )
        | do_parse!(
              alt!(tag_no_case!("key") | tag_no_case!("index")) >>
              opt_multispace >>
              name: sql_identifier >>
              opt_multispace >>
              columns: delimited!(tag!("("), delimited!(opt_multispace, index_col_list, opt_multispace), tag!(")")) >>
              ({
                  let n = String::from_utf8(name.to_vec()).unwrap();
                  TableKey::Key(n, columns)
              })
          )
    )
);

/// Parse rule for a comma-separated list.
named!(pub key_specification_list<CompleteByteSlice, Vec<TableKey>>,
       many1!(
           do_parse!(
               key: key_specification >>
               opt!(
                   do_parse!(
                       opt_multispace >>
                       tag!(",") >>
                       opt_multispace >>
                       ()
                   )
               ) >>
               (key)
           )
       )
);

/// Parse rule for a comma-separated list.
named!(pub field_specification_list<CompleteByteSlice, Vec<ColumnSpecification> >,
       many1!(
           do_parse!(
               identifier: column_identifier_no_alias >>
               fieldtype: opt!(do_parse!(multispace >>
                                      ti: type_identifier >>
                                      opt_multispace >>
                                      (ti)
                               )
               ) >>
               constraints: many0!(column_constraint) >>
               comment: opt!(parse_comment) >>
               opt!(
                   do_parse!(
                       opt_multispace >>
                       tag!(",") >>
                       opt_multispace >>
                       ()
                   )
               ) >>
               ({
                   let t = match fieldtype {
                       None => SqlType::Text,
                       Some(ref t) => t.clone(),
                   };
                   ColumnSpecification {
                       column: identifier,
                       sql_type: t,
                       constraints: constraints.into_iter().filter_map(|m|m).collect(),
                       comment: comment,
                   }
               })
           )
       )
);

/// Parse rule for a column definition contraint.
named!(pub column_constraint<CompleteByteSlice, Option<ColumnConstraint>>,
    alt!(
          do_parse!(
              opt_multispace >>
              tag_no_case!("not null") >>
              opt_multispace >>
              (Some(ColumnConstraint::NotNull))
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("null") >>
              opt_multispace >>
              (None)
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("auto_increment") >>
              opt_multispace >>
              (Some(ColumnConstraint::AutoIncrement))
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("default") >>
              multispace >>
              def: alt!(
                    do_parse!(s: delimited!(tag!("'"), take_until!("'"), tag!("'")) >> (
                        Literal::String(String::from_utf8(s.to_vec()).unwrap())
                    ))
                  | do_parse!(i: digit >>
                              tag!(".") >>
                              f: digit >> (
                              Literal::FixedPoint(Real {
                                  integral: i32::from_str(str::from_utf8(*i).unwrap()).unwrap(),
                                  fractional: i32::from_str(str::from_utf8(*f).unwrap()).unwrap()
                              })
                    ))
                  | do_parse!(d: digit >> (
                        Literal::Integer(i64::from_str(str::from_utf8(*d).unwrap()).unwrap())
                    ))
                  | do_parse!(tag!("''") >> (Literal::String(String::from(""))))
                  | do_parse!(tag_no_case!("null") >> (Literal::Null))
                  | do_parse!(tag_no_case!("current_timestamp") >> (Literal::CurrentTimestamp))
              ) >>
              opt_multispace >>
              (Some(ColumnConstraint::DefaultValue(def)))
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("primary key") >>
              opt_multispace >>
              (Some(ColumnConstraint::PrimaryKey))
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("unique") >>
              opt_multispace >>
              (Some(ColumnConstraint::Unique))
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("character set") >>
              multispace >>
              charset: sql_identifier >>
              (Some(ColumnConstraint::CharacterSet(str::from_utf8(*charset).unwrap().to_owned())))
          )
        | do_parse!(
              opt_multispace >>
              tag_no_case!("collate") >>
              multispace >>
              collation: sql_identifier >>
              (Some(ColumnConstraint::Collation(str::from_utf8(*collation).unwrap().to_owned())))
          )
    )
);

/// Parse rule for a comma-separated list.
named!(pub field_fk_specification_list<CompleteByteSlice, Vec<Column>>,
    many1!(
        do_parse!(
            opt_multispace >>
            column: sql_identifier >>
            opt_multispace >>
            opt!(
                do_parse!(
                    opt_multispace >>
                    tag!(",") >>
                    opt_multispace >>
                    ()
                )
            ) >>
            ({
                Column {
                    name: String::from_utf8(column.to_vec()).unwrap(),
                    alias: None,
                    table: None,
                    function: None,
                }
            })
        )
    )
);

named!(pub foreign_key_ref_action_list<CompleteByteSlice, Vec<String> >,
    many1!(
        do_parse!(
            opt_multispace >>
            tag_no_case!("ON") >>
            multispace >>
            name: alt!(tag_no_case!("DELETE") | tag_no_case!("UPDATE")) >>
            multispace >>
            tag_no_case!("RESTRICT") >>
            ({String::from_utf8(name.to_vec()).unwrap()})
        )
    ));

/// Parse rule for CONSTRAINT FOREIGN KEY list.
named!(pub foreign_key_specification_list<CompleteByteSlice, Vec<ForeignKeySpecification> >,
       many1!(
           do_parse!(
               name: opt!(do_parse!(
                           opt_multispace >>
                           tag_no_case!("CONSTRAINT") >>
                           opt_multispace >>
                           name: sql_identifier >>
                           (name)
                     )) >>
               opt_multispace >>
               tag_no_case!("foreign") >>
               multispace >>
               tag_no_case!("key") >>
               opt_multispace >>
               tag!("(") >>
               fromfields: field_fk_specification_list >>
               tag!(")") >>
               opt_multispace >>
               tag_no_case!("REFERENCES") >>
               multispace >>
               that_table: table_reference >>
               opt_multispace >>
               tag!("(") >>
               tofields: field_fk_specification_list >>
               tag!(")") >>
               ref_act: opt!(do_parse!(
                   act: foreign_key_ref_action_list >>
                   (act)
               )) >>
               opt_multispace >>
               opt!(
                   do_parse!(
                       opt_multispace >>
                       tag!(",") >>
                       opt_multispace >>
                       ()
                   )
               ) >>
               ({
                   let ref_action = if let Some(ref_act) = ref_act {
                        Some(
                            ref_act.into_iter().map(|a| {
                                    format!("ON {} RESTRICT", a)
                                }).collect::<Vec<_>>().join(" ")
                        )
                   } else {
                       None
                   };
                   ForeignKeySpecification {
                       name: if let Some(name) = name {
                           Some(String::from_utf8(name.to_vec()).unwrap())
                       } else {
                           None
                       },
                       ref_action: ref_action,
                       from: fromfields,
                       that_table: that_table,
                       to: tofields,
                   }
               })
           )
       )
);

/// Parse rule for a SQL CREATE TABLE query.
/// TODO(malte): support types, TEMPORARY tables, IF NOT EXISTS, AS stmt
named!(pub creation<CompleteByteSlice, CreateTableStatement>,
    do_parse!(
        tag_no_case!("create") >>
        multispace >>
        tag_no_case!("table") >>
        multispace >>
        table: table_reference >>
        opt_multispace >>
        tag!("(") >>
        opt_multispace >>
        fields: field_specification_list >>
        opt_multispace >>
        keys: opt!(key_specification_list) >>
        opt_multispace >>
        fkeys: opt!(foreign_key_specification_list) >>
        opt_multispace >>
        tag!(")") >>
        opt_multispace >>
        table_options >>
        statement_terminator >>
        ({
            // "table AS alias" isn't legal in CREATE statements
            assert!(table.alias.is_none());
            // attach table names to columns:
            let named_fields = fields
                .into_iter()
                .map(|field| {
                    let column = Column {
                        table: Some(table.name.clone()),
                        ..field.column
                    };

                    ColumnSpecification { column, ..field }
                })
                .collect();

            // and to keys:
            let named_keys = keys.and_then(|ks| {
                Some(
                    ks.into_iter()
                        .map(|key| {
                            let attach_names = |columns: Vec<Column>| {
                                columns
                                    .into_iter()
                                    .map(|column| Column {
                                        table: Some(table.name.clone()),
                                        ..column
                                    })
                                    .collect()
                            };

                            match key {
                                TableKey::PrimaryKey(columns) => {
                                    TableKey::PrimaryKey(attach_names(columns))
                                }
                                TableKey::UniqueKey(name, columns) => {
                                    TableKey::UniqueKey(name, attach_names(columns))
                                }
                                TableKey::FulltextKey(name, columns) => {
                                    TableKey::FulltextKey(name, attach_names(columns))
                                }
                                TableKey::Key(name, columns) => {
                                    TableKey::Key(name, attach_names(columns))
                                }
                            }
                        })
                        .collect(),
                )
            });

            CreateTableStatement {
                table: table,
                fields: named_fields,
                keys: named_keys,
                fkeys: fkeys,
            }
        })
    )
);

/// Parse rule for a SQL CREATE VIEW query.
named!(pub view_creation<CompleteByteSlice, CreateViewStatement>,
    do_parse!(
        tag_no_case!("create") >>
        multispace >>
        tag_no_case!("view") >>
        multispace >>
        name: sql_identifier >>
        multispace >>
        tag_no_case!("as") >>
        multispace >>
        definition: alt!(
              map!(compound_selection, |s| SelectSpecification::Compound(s))
            | map!(nested_selection, |s| SelectSpecification::Simple(s))
        ) >>
        statement_terminator >>
        ({
            CreateViewStatement {
                name: String::from_utf8(name.to_vec()).unwrap(),
                fields: vec![],  // TODO(malte): support
                definition: Box::new(definition),
            }
        })
    )
);

#[cfg(test)]
mod tests {
    use super::*;
    use column::Column;
    use table::Table;

    #[test]
    fn sql_types() {
        let type0 = "bigint(20) unsigned";
        let type1 = "varchar(255) binary";

        let res = type_identifier(CompleteByteSlice(type0.as_bytes()));
        assert_eq!(res.unwrap().1, SqlType::Bigint(20));
        let res = type_identifier(CompleteByteSlice(type1.as_bytes()));
        assert_eq!(res.unwrap().1, SqlType::Varchar(255));
    }

    #[test]
    fn field_spec() {
        // N.B. trailing comma here because field_specification_list! doesn't handle the eof case
        // because it is never validly the end of a query
        let qstring = "id bigint(20), name varchar(255),";

        let res = field_specification_list(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            vec![
                ColumnSpecification::new(Column::from("id"), SqlType::Bigint(20)),
                ColumnSpecification::new(Column::from("name"), SqlType::Varchar(255)),
            ]
        );
    }

    #[test]
    fn simple_create() {
        let qstring = "CREATE TABLE users (id bigint(20), name varchar(255), email varchar(255));";

        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("users"),
                fields: vec![
                    ColumnSpecification::new(Column::from("users.id"), SqlType::Bigint(20)),
                    ColumnSpecification::new(Column::from("users.name"), SqlType::Varchar(255)),
                    ColumnSpecification::new(Column::from("users.email"), SqlType::Varchar(255)),
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn create_without_space_after_tablename() {
        let qstring = "CREATE TABLE t(x integer);";
        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("t"),
                fields: vec![
                    ColumnSpecification::new(Column::from("t.x"), SqlType::Int(32)),
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn mediawiki_create() {
        let qstring = "CREATE TABLE user_newtalk (  user_id int(5) NOT NULL default '0',  user_ip \
                       varchar(40) NOT NULL default '') TYPE=MyISAM;";
        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("user_newtalk"),
                fields: vec![
                    ColumnSpecification::with_constraints(
                        Column::from("user_newtalk.user_id"),
                        SqlType::Int(5),
                        vec![
                            ColumnConstraint::NotNull,
                            ColumnConstraint::DefaultValue(Literal::String(String::from("0"))),
                        ],
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("user_newtalk.user_ip"),
                        SqlType::Varchar(40),
                        vec![
                            ColumnConstraint::NotNull,
                            ColumnConstraint::DefaultValue(Literal::String(String::from(""))),
                        ],
                    ),
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn mediawiki_create2() {
        let qstring = "CREATE TABLE `user` (
                        user_id int unsigned NOT NULL PRIMARY KEY AUTO_INCREMENT,
                        user_name varchar(255) binary NOT NULL default '',
                        user_real_name varchar(255) binary NOT NULL default '',
                        user_password tinyblob NOT NULL,
                        user_newpassword tinyblob NOT NULL,
                        user_newpass_time binary(14),
                        user_email tinytext NOT NULL,
                        user_touched binary(14) NOT NULL default '',
                        user_token binary(32) NOT NULL default '',
                        user_email_authenticated binary(14),
                        user_email_token binary(32),
                        user_email_token_expires binary(14),
                        user_registration binary(14),
                        user_editcount int,
                        user_password_expires varbinary(14) DEFAULT NULL
                       ) ENGINE=, DEFAULT CHARSET=utf8";
        creation(CompleteByteSlice(qstring.as_bytes())).unwrap();
    }

    #[test]
    fn mediawiki_create3() {
        let qstring = "CREATE TABLE `interwiki` (
 iw_prefix varchar(32) NOT NULL,
 iw_url blob NOT NULL,
 iw_api blob NOT NULL,
 iw_wikiid varchar(64) NOT NULL,
 iw_local bool NOT NULL,
 iw_trans tinyint NOT NULL default 0
 ) ENGINE=, DEFAULT CHARSET=utf8";
        creation(CompleteByteSlice(qstring.as_bytes())).unwrap();
    }

    #[test]
    fn mediawiki_externallinks() {
        let qstring = "CREATE TABLE `externallinks` (
          `el_id` int(10) unsigned NOT NULL AUTO_INCREMENT,
          `el_from` int(8) unsigned NOT NULL DEFAULT '0',
          `el_from_namespace` int(11) NOT NULL DEFAULT '0',
          `el_to` blob NOT NULL,
          `el_index` blob NOT NULL,
          `el_index_60` varbinary(60) NOT NULL,
          PRIMARY KEY (`el_id`),
          KEY `el_from` (`el_from`,`el_to`(40)),
          KEY `el_to` (`el_to`(60),`el_from`),
          KEY `el_index` (`el_index`(60)),
          KEY `el_backlinks_to` (`el_from_namespace`,`el_to`(60),`el_from`),
          KEY `el_index_60` (`el_index_60`,`el_id`),
          KEY `el_from_index_60` (`el_from`,`el_index_60`,`el_id`)
        )";
        creation(CompleteByteSlice(qstring.as_bytes())).unwrap();
    }

    #[test]
    fn keys() {
        // simple primary key
        let qstring = "CREATE TABLE users (id bigint(20), name varchar(255), email varchar(255), \
                       PRIMARY KEY (id));";

        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("users"),
                fields: vec![
                    ColumnSpecification::new(Column::from("users.id"), SqlType::Bigint(20)),
                    ColumnSpecification::new(Column::from("users.name"), SqlType::Varchar(255)),
                    ColumnSpecification::new(Column::from("users.email"), SqlType::Varchar(255)),
                ],
                keys: Some(vec![TableKey::PrimaryKey(vec![Column::from("users.id")])]),
                ..Default::default()
            }
        );

        // named unique key
        let qstring = "CREATE TABLE users (id bigint(20), name varchar(255), email varchar(255), \
                       UNIQUE KEY id_k (id));";

        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("users"),
                fields: vec![
                    ColumnSpecification::new(Column::from("users.id"), SqlType::Bigint(20)),
                    ColumnSpecification::new(Column::from("users.name"), SqlType::Varchar(255)),
                    ColumnSpecification::new(Column::from("users.email"), SqlType::Varchar(255)),
                ],
                keys: Some(vec![TableKey::UniqueKey(
                    Some(String::from("id_k")),
                    vec![Column::from("users.id")],
                ), ]),
                ..Default::default()
            }
        );
    }

    #[test]
    fn django_create() {
        let qstring = "CREATE TABLE `django_admin_log` (
                       `id` integer AUTO_INCREMENT NOT NULL PRIMARY KEY,
                       `action_time` datetime NOT NULL,
                       `user_id` integer NOT NULL,
                       `content_type_id` integer,
                       `object_id` longtext,
                       `object_repr` varchar(200) NOT NULL,
                       `action_flag` smallint UNSIGNED NOT NULL,
                       `change_message` longtext NOT NULL);";
        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("django_admin_log"),
                fields: vec![
                    ColumnSpecification::with_constraints(
                        Column::from("django_admin_log.id"),
                        SqlType::Int(32),
                        vec![
                            ColumnConstraint::AutoIncrement,
                            ColumnConstraint::NotNull,
                            ColumnConstraint::PrimaryKey,
                        ],
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("django_admin_log.action_time"),
                        SqlType::DateTime(0),
                        vec![ColumnConstraint::NotNull],
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("django_admin_log.user_id"),
                        SqlType::Int(32),
                        vec![ColumnConstraint::NotNull],
                    ),
                    ColumnSpecification::new(
                        Column::from("django_admin_log.content_type_id"),
                        SqlType::Int(32),
                    ),
                    ColumnSpecification::new(
                        Column::from("django_admin_log.object_id"),
                        SqlType::Longtext,
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("django_admin_log.object_repr"),
                        SqlType::Varchar(200),
                        vec![ColumnConstraint::NotNull],
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("django_admin_log.action_flag"),
                        SqlType::Int(32),
                        vec![ColumnConstraint::NotNull],
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("django_admin_log.change_message"),
                        SqlType::Longtext,
                        vec![ColumnConstraint::NotNull],
                    ),
                ],
                ..Default::default()
            }
        );

        let qstring = "CREATE TABLE `auth_group` (
                       `id` integer AUTO_INCREMENT NOT NULL PRIMARY KEY,
                       `name` varchar(80) NOT NULL UNIQUE)";
        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateTableStatement {
                table: Table::from("auth_group"),
                fields: vec![
                    ColumnSpecification::with_constraints(
                        Column::from("auth_group.id"),
                        SqlType::Int(32),
                        vec![
                            ColumnConstraint::AutoIncrement,
                            ColumnConstraint::NotNull,
                            ColumnConstraint::PrimaryKey,
                        ],
                    ),
                    ColumnSpecification::with_constraints(
                        Column::from("auth_group.name"),
                        SqlType::Varchar(80),
                        vec![ColumnConstraint::NotNull, ColumnConstraint::Unique],
                    ),
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn format_create() {
        let qstring = "CREATE TABLE `auth_group` (
                       `id` integer AUTO_INCREMENT NOT NULL PRIMARY KEY,
                       `name` varchar(80) NOT NULL UNIQUE)";
        // TODO(malte): INTEGER isn't quite reflected right here, perhaps
        let expected = "CREATE TABLE auth_group (\
                        id INT(32) AUTO_INCREMENT NOT NULL PRIMARY KEY, \
                        name VARCHAR(80) NOT NULL UNIQUE)";
        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(format!("{}", res.unwrap().1), expected);
    }

    #[test]
    fn simple_create_view() {
        use common::{FieldDefinitionExpression, Operator};
        use condition::{ConditionBase, ConditionExpression, ConditionTree};

        let qstring = "CREATE VIEW v AS SELECT * FROM users WHERE username = \"bob\";";

        let res = view_creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateViewStatement {
                name: String::from("v"),
                fields: vec![],
                definition: Box::new(SelectSpecification::Simple(SelectStatement {
                    tables: vec![Table::from("users")],
                    fields: vec![FieldDefinitionExpression::All],
                    where_clause: Some(ConditionExpression::ComparisonOp(ConditionTree {
                        left: Box::new(ConditionExpression::Base(ConditionBase::Field(
                            "username".into()
                        ))),
                        right: Box::new(ConditionExpression::Base(ConditionBase::Literal(
                            Literal::String("bob".into())
                        ))),
                        operator: Operator::Equal,
                    })),
                    ..Default::default()
                })),
            }
        );
    }

    #[test]
    fn compound_create_view() {
        use common::FieldDefinitionExpression;
        use compound_select::{CompoundSelectOperator, CompoundSelectStatement};

        let qstring = "CREATE VIEW v AS SELECT * FROM users UNION SELECT * FROM old_users;";

        let res = view_creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(
            res.unwrap().1,
            CreateViewStatement {
                name: String::from("v"),
                fields: vec![],
                definition: Box::new(SelectSpecification::Compound(CompoundSelectStatement {
                    selects: vec![
                        (
                            None,
                            SelectStatement {
                                tables: vec![Table::from("users")],
                                fields: vec![FieldDefinitionExpression::All],
                                ..Default::default()
                            },
                        ),
                        (
                            Some(CompoundSelectOperator::DistinctUnion),
                            SelectStatement {
                                tables: vec![Table::from("old_users")],
                                fields: vec![FieldDefinitionExpression::All],
                                ..Default::default()
                            },
                        ),
                    ],
                    order: None,
                    limit: None,
                })),
            }
        );
    }

    #[test]
    fn format_create_view() {
        let qstring = "CREATE VIEW `v` AS SELECT * FROM `t`;";
        let expected = "CREATE VIEW v AS SELECT * FROM t";
        let res = view_creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(format!("{}", res.unwrap().1), expected);
    }

    #[test]
    fn table_foreign_key_spec() {
        let qstring = "FOREIGN KEY(this1, this2) REFERENCES that_table(that1, that2),FOREIGN KEY(this3) REFERENCES that_table2(that3),";

        let res = foreign_key_specification_list(CompleteByteSlice(qstring.as_bytes()));
        let resclone = res.clone();
        println!("{:?}", resclone);
        assert_eq!(
            res.unwrap().1,
            vec![
                ForeignKeySpecification::new(None, None, vec![Column::from("this1"), Column::from("this2")], Table::from("that_table"), vec![Column::from("that1"), Column::from("that2")]),
                ForeignKeySpecification::new(None, None, vec![Column::from("this3")], Table::from("that_table2"), vec![Column::from("that3")]),
            ]
        );
    }

    #[test]
    fn format_create_with_foreign_key() {
        let qstring = "CREATE TABLE `auth_group` (
                       `id` integer AUTO_INCREMENT NOT NULL PRIMARY KEY,
                       `name` varchar(80) NOT NULL UNIQUE,
                       FOREIGN KEY(`name`) REFERENCES artist(`name`))";
        let expected = "CREATE TABLE auth_group (\
                        id INT(32) AUTO_INCREMENT NOT NULL PRIMARY KEY, \
                        name VARCHAR(80) NOT NULL UNIQUE, \
                        FOREIGN KEY(name) REFERENCES artist(name))";
        let res = creation(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(format!("{}", res.unwrap().1), expected);
    }

    #[test]
    fn foreign_key() {
        let qstring = "FOREIGN KEY(`name`) REFERENCES artist(`name`)";
        let expected = "FOREIGN KEY(name) REFERENCES artist(name)";
        let res = foreign_key_specification_list(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(format!("{}", res.unwrap().1[0]), expected);
    }

    #[test]
    fn foreign_key2() {
        let qstring = "FOREIGN KEY   (   `name`   )    REFERENCES   artist    (  `name`  )";
        let expected = "FOREIGN KEY(name) REFERENCES artist(name)";
        let res = foreign_key_specification_list(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(format!("{}", res.unwrap().1[0]), expected);
    }

    #[test]
    fn foreign_key3() {
        let qstring = "CONSTRAINT fk_name FOREIGN KEY(`name`) REFERENCES artist(`name`)";
        let expected = "CONSTRAINT fk_name FOREIGN KEY(name) REFERENCES artist(name)";
        let res = foreign_key_specification_list(CompleteByteSlice(qstring.as_bytes()));
        assert_eq!(format!("{}", res.unwrap().1[0]), expected);
    }
}

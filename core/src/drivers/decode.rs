//! Dynamic row → JSON decoding. sqlx has no generic "give me this cell as
//! a Value" so each engine matches on its own type names and falls back to
//! a string (or a placeholder) for anything exotic.

use serde_json::Value;
use sqlx::{Column, Row, TypeInfo, ValueRef};

fn num_or_string(s: String) -> Value {
    if let Ok(i) = s.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(s)
}

macro_rules! is_null {
    ($row:expr, $idx:expr) => {
        match $row.try_get_raw($idx) {
            Ok(raw) => raw.is_null(),
            Err(_) => false,
        }
    };
}

macro_rules! try_get {
    ($row:expr, $idx:expr, $t:ty) => {
        if let Ok(v) = $row.try_get::<Option<$t>, _>($idx) {
            return v.map(Value::from).unwrap_or(Value::Null);
        }
    };
}

macro_rules! try_get_str {
    ($row:expr, $idx:expr, $t:ty) => {
        if let Ok(v) = $row.try_get::<Option<$t>, _>($idx) {
            return v.map(|x| Value::String(x.to_string())).unwrap_or(Value::Null);
        }
    };
}

// ---------------------------------------------------------------- Postgres

pub fn pg_value(row: &sqlx::postgres::PgRow, idx: usize) -> Value {
    if is_null!(row, idx) {
        return Value::Null;
    }
    let ty = row.columns()[idx].type_info().name().to_ascii_uppercase();
    match ty.as_str() {
        "BOOL" => {
            try_get!(row, idx, bool);
        }
        "INT2" | "SMALLINT" => {
            try_get!(row, idx, i16);
        }
        "INT4" | "INT" | "SERIAL" | "INTEGER" => {
            try_get!(row, idx, i32);
        }
        "INT8" | "BIGINT" | "BIGSERIAL" => {
            try_get!(row, idx, i64);
        }
        "FLOAT4" | "REAL" => {
            try_get!(row, idx, f32);
        }
        "FLOAT8" | "DOUBLE PRECISION" => {
            try_get!(row, idx, f64);
        }
        "NUMERIC" | "DECIMAL" | "MONEY" => {
            if let Ok(v) = row.try_get::<Option<rust_decimal::Decimal>, _>(idx) {
                return v.map(|d| num_or_string(d.to_string())).unwrap_or(Value::Null);
            }
        }
        "JSON" | "JSONB" => {
            if let Ok(v) = row.try_get::<Option<Value>, _>(idx) {
                return v.unwrap_or(Value::Null);
            }
        }
        "UUID" => {
            try_get_str!(row, idx, uuid::Uuid);
        }
        "TIMESTAMPTZ" => {
            if let Ok(v) = row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(idx) {
                return v.map(|d| Value::String(d.to_rfc3339())).unwrap_or(Value::Null);
            }
        }
        "TIMESTAMP" => {
            try_get_str!(row, idx, chrono::NaiveDateTime);
        }
        "DATE" => {
            try_get_str!(row, idx, chrono::NaiveDate);
        }
        "TIME" => {
            try_get_str!(row, idx, chrono::NaiveTime);
        }
        "BYTEA" => {
            if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
                return v
                    .map(|b| Value::String(format!("\\x{}", hex(&b))))
                    .unwrap_or(Value::Null);
            }
        }
        _ => {}
    }
    // text-ish and everything else
    try_get!(row, idx, String);
    try_get_str!(row, idx, uuid::Uuid);
    Value::String(format!("<{ty}>"))
}

// ------------------------------------------------------------------- MySQL

pub fn my_value(row: &sqlx::mysql::MySqlRow, idx: usize) -> Value {
    if is_null!(row, idx) {
        return Value::Null;
    }
    let ty = row.columns()[idx].type_info().name().to_ascii_uppercase();
    match ty.as_str() {
        "BOOLEAN" | "BOOL" => {
            try_get!(row, idx, bool);
        }
        "TINYINT" => {
            try_get!(row, idx, i8);
        }
        "SMALLINT" | "YEAR" => {
            try_get!(row, idx, i16);
        }
        "INT" | "MEDIUMINT" => {
            try_get!(row, idx, i32);
        }
        "BIGINT" => {
            try_get!(row, idx, i64);
        }
        "FLOAT" => {
            try_get!(row, idx, f32);
        }
        "DOUBLE" => {
            try_get!(row, idx, f64);
        }
        "DECIMAL" | "NEWDECIMAL" => {
            if let Ok(v) = row.try_get::<Option<rust_decimal::Decimal>, _>(idx) {
                return v.map(|d| num_or_string(d.to_string())).unwrap_or(Value::Null);
            }
        }
        "JSON" => {
            if let Ok(v) = row.try_get::<Option<Value>, _>(idx) {
                return v.unwrap_or(Value::Null);
            }
        }
        "DATETIME" | "TIMESTAMP" => {
            try_get_str!(row, idx, chrono::NaiveDateTime);
        }
        "DATE" => {
            try_get_str!(row, idx, chrono::NaiveDate);
        }
        "TIME" => {
            try_get_str!(row, idx, chrono::NaiveTime);
        }
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY" => {
            if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
                return v
                    .map(|b| Value::String(format!("0x{}", hex(&b))))
                    .unwrap_or(Value::Null);
            }
        }
        _ => {}
    }
    try_get!(row, idx, String);
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return v
            .map(|b| Value::String(String::from_utf8_lossy(&b).into_owned()))
            .unwrap_or(Value::Null);
    }
    Value::String(format!("<{ty}>"))
}

// ------------------------------------------------------------------ SQLite

pub fn sqlite_value(row: &sqlx::sqlite::SqliteRow, idx: usize) -> Value {
    if is_null!(row, idx) {
        return Value::Null;
    }
    let ty = row.columns()[idx].type_info().name().to_ascii_uppercase();
    match ty.as_str() {
        "BOOLEAN" => {
            try_get!(row, idx, bool);
        }
        "INTEGER" | "INT" | "BIGINT" => {
            try_get!(row, idx, i64);
        }
        "REAL" | "FLOAT" | "DOUBLE" => {
            try_get!(row, idx, f64);
        }
        "BLOB" => {
            if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
                return v
                    .map(|b| Value::String(format!("0x{}", hex(&b))))
                    .unwrap_or(Value::Null);
            }
        }
        _ => {}
    }
    try_get!(row, idx, String);
    try_get!(row, idx, i64);
    try_get!(row, idx, f64);
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return v
            .map(|b| Value::String(String::from_utf8_lossy(&b).into_owned()))
            .unwrap_or(Value::Null);
    }
    Value::String(format!("<{ty}>"))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes.iter().take(1024) {
        s.push_str(&format!("{b:02x}"));
    }
    if bytes.len() > 1024 {
        s.push('…');
    }
    s
}

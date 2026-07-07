use crate::sql::models::DbConnection;

pub fn find_connection<'a>(nick: &str, conns: &'a [DbConnection]) -> Option<&'a DbConnection> {
    conns.iter().find(|c| c.nickname == nick)
}

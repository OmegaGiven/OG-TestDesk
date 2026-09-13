//! Shared `run_query` body. The three drivers differ only by pool type and
//! per-cell decode fn, so the paging + counting + streaming logic lives
//! here as a macro.

/// `run_query_body!(cfg_expr, pool_expr, sql, opts, value_fn)` — expands to
/// the full body of a `DbDriver::run_query` impl. Uses `?` and early
/// `return`, so call it as the last expression of the async fn.
macro_rules! run_query_body {
    ($cfg:expr, $pool:expr, $sql:expr, $opts:expr, $valfn:path) => {{
        use futures_util::TryStreamExt;
        use sqlx::{Column as _, Row as _, TypeInfo as _};
        use std::time::{Duration, Instant};
        use $crate::drivers::{
            is_wrappable, max_rows, stmt_returns_rows, strip_trailing_semi, wrap_count, wrap_paged,
            QueryColumn, QueryResult,
        };

        let cfg = $cfg;
        let pool = $pool;
        let sql: &str = $sql;
        let opts = $opts;
        let start = Instant::now();

        if cfg.read_only && !stmt_returns_rows(sql) {
            return Err(anyhow::anyhow!(
                "connection '{}' is read-only — only row-returning statements are allowed",
                cfg.nickname
            ));
        }

        if !stmt_returns_rows(sql) {
            let res = sqlx::query(sql).execute(&pool).await?;
            return Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                row_count: 0,
                rows_affected: res.rows_affected(),
                duration_ms: start.elapsed().as_millis() as u64,
                is_select: false,
                truncated: false,
                page: 0,
                page_size: 0,
                total: None,
                count_ms: None,
                has_more: false,
            });
        }

        let wrappable = is_wrappable(sql);
        let page_limit = opts.limit.filter(|_| wrappable);
        let exec_sql = match page_limit {
            Some(lim) => wrap_paged(sql, lim, opts.offset),
            None => strip_trailing_semi(sql).to_string(),
        };

        // --- page fetch (streamed, hard-capped) ---
        let page_fut = async {
            let cap = opts.row_cap_override.unwrap_or_else(max_rows);
            let mut stream = sqlx::query(&exec_sql).fetch(&pool);
            let mut columns: Vec<QueryColumn> = Vec::new();
            let mut data: Vec<Vec<serde_json::Value>> = Vec::new();
            let mut truncated = false;
            while let Some(row) = stream.try_next().await? {
                if columns.is_empty() {
                    columns = row
                        .columns()
                        .iter()
                        .map(|c| QueryColumn {
                            name: c.name().to_string(),
                            type_name: c.type_info().name().to_string(),
                        })
                        .collect();
                }
                if data.len() >= cap {
                    truncated = true;
                    break;
                }
                data.push((0..row.len()).map(|i| $valfn(&row, i)).collect());
            }
            Ok::<_, anyhow::Error>((columns, data, truncated))
        };

        // --- optional COUNT(*), time-boxed ---
        let count_fut = async {
            if opts.count && wrappable {
                let cstart = Instant::now();
                let n: std::result::Result<i64, _> =
                    sqlx::query_scalar(&wrap_count(sql)).fetch_one(&pool).await;
                n.ok()
                    .map(|v| (v.max(0) as usize, cstart.elapsed().as_millis() as u64))
            } else {
                None
            }
        };
        let count_boxed = tokio::time::timeout(Duration::from_millis(3000), count_fut);

        let (page_res, count_res) = tokio::join!(page_fut, count_boxed);
        let (columns, data, truncated) = page_res?;
        let (total, count_ms) = match count_res {
            Ok(Some((n, ms))) => (Some(n), Some(ms)),
            _ => (None, None),
        };

        let page_size = page_limit.unwrap_or(0);
        let page = match page_limit {
            Some(lim) if lim > 0 => opts.offset / lim,
            _ => 0,
        };
        let n = data.len();
        Ok(QueryResult {
            row_count: n,
            rows_affected: 0,
            columns,
            rows: data,
            duration_ms: start.elapsed().as_millis() as u64,
            is_select: true,
            truncated,
            page,
            page_size,
            total,
            count_ms,
            has_more: page_limit.map(|lim| n >= lim).unwrap_or(false),
        })
    }};
}

pub(crate) use run_query_body;

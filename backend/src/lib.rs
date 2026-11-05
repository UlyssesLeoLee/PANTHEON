use axum::{extract::State, http::{HeaderMap, StatusCode}, routing::{get, post}, Json, Router};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone)]
pub struct AppState { pub db_path: PathBuf, pub api_token: String }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Column { pub name: String, pub data_type: String, pub not_null: bool, pub primary_key: bool }
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ForeignKey { pub from: String, pub to_table: String, pub to: String }
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableInfo { pub name: String, pub columns: Vec<Column>, pub foreign_keys: Vec<ForeignKey> }
#[derive(Debug, Serialize)]
pub struct Schema { pub database: String, pub tables: Vec<TableInfo> }
#[derive(Debug, Deserialize)]
pub struct QueryRequest { pub sql: String, pub limit: Option<usize> }
#[derive(Debug, Serialize)]
pub struct QueryResponse { pub columns: Vec<String>, pub rows: Vec<Vec<serde_json::Value>>, pub truncated: bool }

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Only a single SELECT, WITH or EXPLAIN statement is permitted")]
    ForbiddenSql,
    #[error("Invalid database configuration")]
    InvalidConfig,
    #[error("Database error: {0}")]
    Database(String),
}
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        use axum::response::IntoResponse;
        let status = match self { Self::Unauthorized => StatusCode::UNAUTHORIZED, Self::ForbiddenSql => StatusCode::FORBIDDEN, Self::InvalidConfig => StatusCode::BAD_REQUEST, Self::Database(_) => StatusCode::BAD_REQUEST };
        (status, Json(serde_json::json!({"error": self.to_string()}))).into_response()
    }
}
fn connect(path: &PathBuf) -> Result<Connection, ApiError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| ApiError::Database(e.to_string()))?;
    conn.busy_timeout(Duration::from_secs(2)).map_err(|e|ApiError::Database(e.to_string()))?;
    conn.execute_batch("PRAGMA query_only = ON;").map_err(|e| ApiError::Database(e.to_string()))?;
    conn.progress_handler(1000, Some({ let started=std::time::Instant::now(); move || started.elapsed()>Duration::from_secs(5) }));
    Ok(conn)
}
fn quoted(name:&str)->String { format!("\"{}\"",name.replace('"',"\"\"")) }
fn require_token(h:&HeaderMap, state:&AppState)->Result<(),ApiError>{
    let expected=format!("Bearer {}",state.api_token);
    if h.get("authorization").and_then(|x|x.to_str().ok())==Some(expected.as_str()) {Ok(())}else{Err(ApiError::Unauthorized)}
}
pub fn inspect_db(path: &PathBuf) -> Result<Schema,ApiError> {
    let conn=connect(path)?;
    let mut query=conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(|e|ApiError::Database(e.to_string()))?;
    let names=query.query_map([],|r|r.get::<_,String>(0)).map_err(|e|ApiError::Database(e.to_string()))?
       .collect::<Result<Vec<_>,_>>().map_err(|e|ApiError::Database(e.to_string()))?;
    let mut tables=vec![];
    for name in names {
        let mut stmt=conn.prepare(&format!("PRAGMA table_info({})",quoted(&name))).map_err(|e|ApiError::Database(e.to_string()))?;
        let cols=stmt.query_map([],|r| Ok(Column {name:r.get(1)?,data_type:r.get(2)?,not_null:r.get::<_,i32>(3)?!=0,primary_key:r.get::<_,i32>(5)?!=0}))
            .map_err(|e|ApiError::Database(e.to_string()))?.collect::<Result<Vec<_>,_>>().map_err(|e|ApiError::Database(e.to_string()))?;
        let mut stmt=conn.prepare(&format!("PRAGMA foreign_key_list({})",quoted(&name))).map_err(|e|ApiError::Database(e.to_string()))?;
        let foreign_keys=stmt.query_map([],|r|Ok(ForeignKey{to_table:r.get(2)?,from:r.get(3)?,to:r.get(4)?}))
            .map_err(|e|ApiError::Database(e.to_string()))?.collect::<Result<Vec<_>,_>>().map_err(|e|ApiError::Database(e.to_string()))?;
        tables.push(TableInfo{name,columns:cols,foreign_keys});
    }
    Ok(Schema{database:path.file_name().unwrap_or_default().to_string_lossy().into_owned(),tables})
}
fn value_to_json(v:ValueRef<'_>)->serde_json::Value{
    match v { ValueRef::Null=>serde_json::Value::Null, ValueRef::Integer(n)=>n.into(), ValueRef::Real(n)=>serde_json::json!(n), ValueRef::Text(bytes)=>serde_json::Value::String(String::from_utf8_lossy(bytes).into_owned()), ValueRef::Blob(bytes)=>serde_json::Value::String(format!("[BLOB {} bytes]",bytes.len())) }
}
// Reject separators outside quoted SQL text and comments; allow one final terminator.
fn has_multiple_statements(sql: &str) -> bool {
    let mut chars = sql.chars().peekable();
    let mut quote: Option<char> = None;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut ended = false;
    while let Some(c) = chars.next() {
        if line_comment { if c == '\n' { line_comment=false; } continue; }
        if block_comment { if c == '*' && chars.peek()==Some(&'/') { chars.next(); block_comment=false; } continue; }
        if let Some(q)=quote {
            if c==q { if chars.peek()==Some(&q) { chars.next(); } else { quote=None; } }
            continue;
        }
        if c=='-' && chars.peek()==Some(&'-') { chars.next(); line_comment=true; continue; }
        if c=='/' && chars.peek()==Some(&'*') { chars.next(); block_comment=true; continue; }
        if c=='\'' || c=='"' || c=='`' { quote=Some(c); continue; }
        if c==';' { ended=true; continue; }
        if ended && !c.is_whitespace() { return true; }
    }
    quote.is_some() || block_comment
}
pub fn run_query(path: &PathBuf, sql:&str, requested_limit:Option<usize>)->Result<QueryResponse,ApiError>{
    let sql=sql.trim();
    let verb=sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    if !["SELECT","WITH","EXPLAIN"].contains(&verb.as_str()) {return Err(ApiError::ForbiddenSql)}
    if has_multiple_statements(sql) { return Err(ApiError::ForbiddenSql); }
    let conn=connect(path)?;
    let mut stmt=conn.prepare(sql).map_err(|e|ApiError::Database(e.to_string()))?;
    if !stmt.readonly() || stmt.parameter_count()!=0 {return Err(ApiError::ForbiddenSql)}
    let columns=stmt.column_names().iter().map(|s|s.to_string()).collect::<Vec<_>>();
    let cap=requested_limit.unwrap_or(200).clamp(1,1000);
    let mut rows=stmt.query([]).map_err(|e|ApiError::Database(e.to_string()))?;
    let mut data=Vec::new();
    while let Some(row)=rows.next().map_err(|e|ApiError::Database(e.to_string()))? {
        if data.len()==cap {return Ok(QueryResponse{columns,rows:data,truncated:true})}
        let mut values=Vec::new();
        for i in 0..columns.len(){values.push(value_to_json(row.get_ref(i).map_err(|e|ApiError::Database(e.to_string()))?));}
        data.push(values);
    }
    Ok(QueryResponse{columns,rows:data,truncated:false})
}
async fn health()->Json<serde_json::Value>{Json(serde_json::json!({"status":"ok","product":"PANTHEON","version":"0.1.0"}))}
async fn schema(State(s):State<Arc<AppState>>,headers:HeaderMap)->Result<Json<Schema>,ApiError>{require_token(&headers,&s)?;Ok(Json(inspect_db(&s.db_path)?))}
async fn query(State(s):State<Arc<AppState>>,headers:HeaderMap,Json(request):Json<QueryRequest>)->Result<Json<QueryResponse>,ApiError>{require_token(&headers,&s)?;Ok(Json(run_query(&s.db_path,&request.sql,request.limit)?))}
pub fn router(state:AppState,origin:&str)->Router {
    let origin:axum::http::HeaderValue=origin.parse().expect("PANTHEON_UI_ORIGIN must be a valid header value");
    Router::new().route("/health",get(health)).route("/api/schema",get(schema)).route("/api/query",post(query))
        .layer(CorsLayer::new().allow_origin(AllowOrigin::exact(origin)).allow_headers([axum::http::header::AUTHORIZATION,axum::http::header::CONTENT_TYPE]).allow_methods([axum::http::Method::GET,axum::http::Method::POST]))
        .with_state(Arc::new(state))
}
#[cfg(test)] mod tests {
    use super::*;
    fn sample()->(tempfile::TempDir,PathBuf){let dir=tempfile::tempdir().unwrap();let path=dir.path().join("test.db");let c=Connection::open(&path).unwrap();c.execute_batch("CREATE TABLE users(id INTEGER PRIMARY KEY,name TEXT);INSERT INTO users VALUES(1,'Ada');").unwrap();(dir,path)}
    #[test] fn schema_has_table(){let (_d,p)=sample();let s=inspect_db(&p).unwrap();assert_eq!(s.tables[0].name,"users");}
    #[test] fn read_select(){let (_d,p)=sample();let r=run_query(&p,"SELECT * FROM users",None).unwrap();assert_eq!(r.rows.len(),1);}
    #[test] fn blocked_dml(){let (_d,p)=sample();assert!(matches!(run_query(&p,"DELETE FROM users",None),Err(ApiError::ForbiddenSql)));}
    #[test] fn blocked_cte_write(){let (_d,p)=sample();assert!(run_query(&p,"WITH c AS (SELECT 1) DELETE FROM users",None).is_err());}
    #[test] fn limit_enforced(){let (_d,p)=sample();let r=run_query(&p,"SELECT 1 UNION ALL SELECT 2",Some(1)).unwrap();assert!(r.truncated);}
    #[test] fn deny_multi_statement(){let (_d,p)=sample();assert!(matches!(run_query(&p,"SELECT 1; DELETE FROM users;",None),Err(ApiError::ForbiddenSql)));}
    #[test] fn accept_quoted_semicolon(){let (_d,p)=sample();let r=run_query(&p,"SELECT ';' as value;",None).unwrap();assert_eq!(r.rows.len(),1);}
    #[test] fn no_attach(){let (_d,p)=sample();assert!(run_query(&p,"ATTACH DATABASE '/tmp/a.db' AS a",None).is_err());}
}

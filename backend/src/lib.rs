use axum::{extract::State, http::{HeaderMap, StatusCode}, routing::{get, post}, Json, Router};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::{Path, PathBuf}, sync::Arc, time::Duration};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub mod pithos;

/// Which kind of database the API serves. Both kinds are only ever read.
#[derive(Debug, Clone)]
pub enum Backend { Sqlite(PathBuf), Pithos(PathBuf) }

impl Backend {
    /// Classifies a configured path: directories and WAL files are PITHOS, other files are SQLite.
    pub fn detect(path: &Path) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|e| format!("PANTHEON_DB_PATH {} is not accessible: {e}", path.display()))?;
        if !path.is_file() || pithos::is_wal_name(&path) || pithos::has_wal_magic(&path) {
            Ok(Self::Pithos(path))
        } else {
            Ok(Self::Sqlite(path))
        }
    }
    pub fn kind(&self) -> &'static str { match self { Self::Sqlite(_) => "sqlite", Self::Pithos(_) => "pithos" } }
}

#[derive(Clone)]
pub struct AppState { pub backend: Backend, pub api_token: String }

/// Validated startup configuration read from the `PANTHEON_*` environment variables.
#[derive(Debug, Clone)]
pub struct Settings { pub backend: Backend, pub api_token: String, pub ui_origin: String, pub bind: String }

impl Settings {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let db_path = get("PANTHEON_DB_PATH").filter(|v| !v.trim().is_empty())
            .ok_or_else(|| "Set PANTHEON_DB_PATH to a SQLite database file or a PITHOS data directory".to_string())?;
        let backend = Backend::detect(Path::new(&db_path))?;
        let api_token = get("PANTHEON_API_TOKEN").ok_or_else(|| "Set PANTHEON_API_TOKEN to a random token".to_string())?;
        if api_token.chars().count() < 24 { return Err("PANTHEON_API_TOKEN must contain at least 24 characters".into()); }
        let ui_origin = get("PANTHEON_UI_ORIGIN").unwrap_or_else(|| "http://localhost:3000".into());
        axum::http::HeaderValue::from_str(&ui_origin).map_err(|_| format!("PANTHEON_UI_ORIGIN is not a valid header value: {ui_origin:?}"))?;
        let bind = get("PANTHEON_API_BIND").unwrap_or_else(|| "127.0.0.1:8788".into());
        Ok(Self { backend, api_token, ui_origin, bind })
    }
}

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
    #[error("{0}")]
    Unsupported(String),
}
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = match self { Self::Unauthorized => StatusCode::UNAUTHORIZED, Self::ForbiddenSql => StatusCode::FORBIDDEN, Self::InvalidConfig | Self::Database(_) | Self::Unsupported(_) => StatusCode::BAD_REQUEST };
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
/// Runs blocking database work on tokio's blocking pool so file reads never stall async workers.
async fn blocking<T: Send + 'static>(job: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(job).await.map_err(|e| ApiError::Database(format!("worker task failed: {e}")))?
}
async fn health()->Json<serde_json::Value>{Json(serde_json::json!({"status":"ok","product":"PANTHEON","version":"0.1.0"}))}
async fn schema(State(s):State<Arc<AppState>>,headers:HeaderMap)->Result<Json<Schema>,ApiError>{
    require_token(&headers,&s)?;
    let backend=s.backend.clone();
    Ok(Json(blocking(move||match backend{Backend::Sqlite(path)=>inspect_db(&path),Backend::Pithos(path)=>pithos::inspect(&path)}).await?))
}
async fn query(State(s):State<Arc<AppState>>,headers:HeaderMap,Json(request):Json<QueryRequest>)->Result<Json<QueryResponse>,ApiError>{
    require_token(&headers,&s)?;
    let backend=s.backend.clone();
    Ok(Json(blocking(move||match backend{Backend::Sqlite(path)=>run_query(&path,&request.sql,request.limit),Backend::Pithos(path)=>pithos::run_query(&path,&request.sql,request.limit)}).await?))
}
pub fn router(state:AppState,origin:&str)->Router {
    let origin:axum::http::HeaderValue=origin.parse().expect("PANTHEON_UI_ORIGIN must be a valid header value");
    Router::new().route("/health",get(health)).route("/api/schema",get(schema)).route("/api/query",post(query))
        .layer(CorsLayer::new().allow_origin(AllowOrigin::exact(origin)).allow_headers([axum::http::header::AUTHORIZATION,axum::http::header::CONTENT_TYPE]).allow_methods([axum::http::Method::GET,axum::http::Method::POST]))
        .with_state(Arc::new(state))
}
/// Serves the API on a bound listener until `shutdown` completes, then stops gracefully.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState, origin: &str, shutdown: impl Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    axum::serve(listener, router(state, origin)).with_graceful_shutdown(shutdown).await
}
/// Reads settings from `get`, binds the listener, prints the banner and serves until `shutdown`.
pub async fn run_with_shutdown(get: impl Fn(&str) -> Option<String>, shutdown: impl Future<Output = ()> + Send + 'static) -> Result<(), String> {
    let settings = Settings::from_lookup(get)?;
    let listener = tokio::net::TcpListener::bind(&settings.bind).await.map_err(|e| format!("cannot bind {}: {e}", settings.bind))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    println!("PANTHEON API on http://{address} (backend: {})", settings.backend.kind());
    let Settings { backend, api_token, ui_origin, .. } = settings;
    serve(listener, AppState { backend, api_token }, &ui_origin, shutdown).await.map_err(|e| format!("server stopped: {e}"))
}
/// Process entry: configuration from the environment, stops on Ctrl+C.
pub async fn run() -> Result<(), String> {
    run_with_shutdown(|name| std::env::var(name).ok(), async { let _ = tokio::signal::ctrl_c().await; }).await
}
#[cfg(test)] mod tests {
    use super::*;
    use std::collections::HashMap;
    fn sample()->(tempfile::TempDir,PathBuf){let dir=tempfile::tempdir().unwrap();let path=dir.path().join("test.db");let c=Connection::open(&path).unwrap();c.execute_batch("CREATE TABLE users(id INTEGER PRIMARY KEY,name TEXT);INSERT INTO users VALUES(1,'Ada');").unwrap();(dir,path)}
    #[test] fn schema_has_table(){let (_d,p)=sample();let s=inspect_db(&p).unwrap();assert_eq!(s.tables[0].name,"users");}
    #[test] fn read_select(){let (_d,p)=sample();let r=run_query(&p,"SELECT * FROM users",None).unwrap();assert_eq!(r.rows.len(),1);}
    #[test] fn blocked_dml(){let (_d,p)=sample();assert!(matches!(run_query(&p,"DELETE FROM users",None),Err(ApiError::ForbiddenSql)));}
    #[test] fn blocked_cte_write(){let (_d,p)=sample();assert!(run_query(&p,"WITH c AS (SELECT 1) DELETE FROM users",None).is_err());}
    #[test] fn limit_enforced(){let (_d,p)=sample();let r=run_query(&p,"SELECT 1 UNION ALL SELECT 2",Some(1)).unwrap();assert!(r.truncated);}
    #[test] fn deny_multi_statement(){let (_d,p)=sample();assert!(matches!(run_query(&p,"SELECT 1; DELETE FROM users;",None),Err(ApiError::ForbiddenSql)));}
    #[test] fn accept_quoted_semicolon(){let (_d,p)=sample();let r=run_query(&p,"SELECT ';' as value;",None).unwrap();assert_eq!(r.rows.len(),1);}
    #[test] fn no_attach(){let (_d,p)=sample();assert!(run_query(&p,"ATTACH DATABASE '/tmp/a.db' AS a",None).is_err());}

    fn lookup(pairs: &[(&str, String)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        move |name| map.get(name).cloned()
    }
    fn path_text(path: &Path) -> String { path.to_string_lossy().into_owned() }
    const TOKEN: &str = "0123456789abcdef01234567";

    #[test] fn detect_backend_kinds(){
        let dir=tempfile::tempdir().unwrap();
        assert!(matches!(Backend::detect(dir.path()),Ok(Backend::Pithos(_))));
        let wal=dir.path().join("x.wal");std::fs::write(&wal,b"").unwrap();
        assert!(matches!(Backend::detect(&wal),Ok(Backend::Pithos(_))));
        let upper=dir.path().join("y.WAL");std::fs::write(&upper,b"").unwrap();
        assert!(matches!(Backend::detect(&upper),Ok(Backend::Pithos(_))));
        let magic=dir.path().join("log.bin");std::fs::write(&magic,b"PDB1....").unwrap();
        assert!(matches!(Backend::detect(&magic),Ok(Backend::Pithos(_))));
        let (_d,sqlite)=sample();
        assert!(matches!(Backend::detect(&sqlite),Ok(Backend::Sqlite(_))));
        let empty=dir.path().join("empty");std::fs::write(&empty,b"").unwrap();
        assert!(matches!(Backend::detect(&empty),Ok(Backend::Sqlite(_))));
        let missing=dir.path().join("missing");
        let error=Backend::detect(&missing).unwrap_err();
        assert!(error.contains("missing") && error.contains("PANTHEON_DB_PATH"),"{error}");
        assert_eq!(Backend::Sqlite(PathBuf::new()).kind(),"sqlite");
        assert_eq!(Backend::Pithos(PathBuf::new()).kind(),"pithos");
    }
    #[test] fn settings_defaults_and_kinds(){
        let (_d,sqlite)=sample();
        let s=Settings::from_lookup(lookup(&[("PANTHEON_DB_PATH",path_text(&sqlite)),("PANTHEON_API_TOKEN",TOKEN.into())])).unwrap();
        assert!(matches!(s.backend,Backend::Sqlite(_)));
        assert_eq!((s.ui_origin.as_str(),s.bind.as_str(),s.api_token.as_str()),("http://localhost:3000","127.0.0.1:8788",TOKEN));
        let dir=tempfile::tempdir().unwrap();
        let s=Settings::from_lookup(lookup(&[("PANTHEON_DB_PATH",path_text(dir.path())),("PANTHEON_API_TOKEN",TOKEN.into())])).unwrap();
        assert!(matches!(s.backend,Backend::Pithos(_)));
    }
    #[test] fn settings_explicit_values_and_minimum_token(){
        let dir=tempfile::tempdir().unwrap();
        let s=Settings::from_lookup(lookup(&[("PANTHEON_DB_PATH",path_text(dir.path())),("PANTHEON_API_TOKEN",TOKEN.into()),("PANTHEON_UI_ORIGIN","http://localhost:4000".into()),("PANTHEON_API_BIND","127.0.0.1:9999".into())])).unwrap();
        assert_eq!(TOKEN.len(),24);
        assert_eq!((s.ui_origin.as_str(),s.bind.as_str()),("http://localhost:4000","127.0.0.1:9999"));
    }
    #[test] fn settings_rejects_bad_configuration(){
        let dir=tempfile::tempdir().unwrap();
        let valid=path_text(dir.path());
        let err=|pairs:&[(&str,String)]|Settings::from_lookup(lookup(pairs)).unwrap_err();
        assert!(err(&[]).contains("PANTHEON_DB_PATH"));
        assert!(err(&[("PANTHEON_DB_PATH","   ".into())]).contains("PANTHEON_DB_PATH"));
        let missing=path_text(&dir.path().join("nope"));
        assert!(err(&[("PANTHEON_DB_PATH",missing.clone()),("PANTHEON_API_TOKEN",TOKEN.into())]).contains("nope"));
        assert!(err(&[("PANTHEON_DB_PATH",valid.clone())]).contains("PANTHEON_API_TOKEN"));
        assert!(err(&[("PANTHEON_DB_PATH",valid.clone()),("PANTHEON_API_TOKEN",TOKEN[..23].into())]).contains("at least 24"));
        assert!(err(&[("PANTHEON_DB_PATH",valid),("PANTHEON_API_TOKEN",TOKEN.into()),("PANTHEON_UI_ORIGIN","http://bad\norigin".into())]).contains("PANTHEON_UI_ORIGIN"));
    }
    #[tokio::test] async fn blocking_maps_worker_failures(){
        let ok=blocking(||Ok::<_,ApiError>(7)).await.unwrap();
        assert_eq!(ok,7);
        let passthrough=blocking(||Err::<(),_>(ApiError::ForbiddenSql)).await;
        assert!(matches!(passthrough,Err(ApiError::ForbiddenSql)));
        let panicked=blocking(||->Result<(),ApiError>{panic!("boom")}).await;
        assert!(matches!(&panicked,Err(ApiError::Database(m)) if m.contains("worker task failed")),"{panicked:?}");
    }
    #[tokio::test] async fn api_error_status_and_body(){
        use axum::response::IntoResponse;
        let cases=[(ApiError::Unauthorized,401,"Unauthorized"),(ApiError::ForbiddenSql,403,"Only a single SELECT, WITH or EXPLAIN statement is permitted"),(ApiError::InvalidConfig,400,"Invalid database configuration"),(ApiError::Database("x".into()),400,"Database error: x"),(ApiError::Unsupported("nope".into()),400,"nope")];
        for (error,status,message) in cases {
            let response=error.into_response();
            assert_eq!(response.status().as_u16(),status);
            let body=axum::body::to_bytes(response.into_body(),usize::MAX).await.unwrap();
            let json:serde_json::Value=serde_json::from_slice(&body).unwrap();
            assert_eq!(json,serde_json::json!({"error":message}));
        }
    }
}

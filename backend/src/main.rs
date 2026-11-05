use pantheon_api::{router,AppState};
use std::{env,path::PathBuf};
#[tokio::main]
async fn main(){
    tracing_subscriber::fmt::init();
    let path=PathBuf::from(env::var("PANTHEON_DB_PATH").expect("Set PANTHEON_DB_PATH to an existing SQLite database"));
    let path=path.canonicalize().expect("Database file must exist");
    assert!(path.is_file(),"PANTHEON_DB_PATH must be a file");
    let token=env::var("PANTHEON_API_TOKEN").expect("Set PANTHEON_API_TOKEN to a random token");
    assert!(token.len()>=24,"PANTHEON_API_TOKEN must contain >=24 characters");
    let origin=env::var("PANTHEON_UI_ORIGIN").unwrap_or("http://localhost:3000".into());
    let addr=env::var("PANTHEON_API_BIND").unwrap_or("127.0.0.1:8788".into());
    let listener=tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    println!("PANTHEON API on http://{}",addr);
    axum::serve(listener,router(AppState{db_path:path,api_token:token},&origin)).await.expect("server stopped");
}

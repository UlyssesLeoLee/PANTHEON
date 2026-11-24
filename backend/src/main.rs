use std::process::ExitCode;
#[tokio::main]
async fn main()->ExitCode{
    tracing_subscriber::fmt::init();
    match pantheon_api::run().await {
        Ok(())=>ExitCode::SUCCESS,
        Err(message)=>{
            eprintln!("PANTHEON API failed to start: {message}");
            ExitCode::FAILURE
        }
    }
}

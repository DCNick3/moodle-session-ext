use crate::moodle::SessionProbeResult;
use crate::{config, Database, Moodle};
use actix_web::{post, web, App, HttpServer, Responder, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};
use tracing_actix_web::TracingLogger;

#[derive(Clone)]
struct Data {
    db: Arc<Database>,
    moodle: Arc<Moodle>,
}

#[derive(Deserialize)]
struct ExtendRequest {
    pub moodle_session: String,
}

#[derive(Serialize)]
struct ExtendResponse {
    pub result: bool,
}

fn wrap_result<T>(result: anyhow::Result<T>) -> Result<T> {
    result.map_err(|e| {
        error!("Encountered an error: {}", e);
        actix_web::error::ErrorInternalServerError(e)
    })
}

#[post("/extend-token")]
async fn extend_token(
    data: web::Data<Data>,
    request: web::Json<ExtendRequest>,
) -> Result<impl Responder> {
    let moodle_session = &request.moodle_session;

    let result = match wrap_result(data.moodle.check_session(moodle_session).await)? {
        SessionProbeResult::Invalid => {
            info!("Moodle session {} is invalid", moodle_session);
            false
        }
        SessionProbeResult::Valid {
            email,
            csrf_session,
        } => {
            info!("Provided token is valid, adding to database");
            wrap_result(data.db.add_token(&email, moodle_session, &csrf_session))?;
            true
        }
    };

    Ok(web::Json(ExtendResponse { result }))
}

pub async fn run(
    db: Arc<Database>,
    moodle: Arc<Moodle>,
    config: config::Server,
) -> anyhow::Result<()> {
    let data = Data { db, moodle };

    let mut http = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(data.clone()))
            .wrap(TracingLogger::default())
            .service(extend_token)
    });
    for endpoint in config.endpoints {
        http = http.bind(endpoint)?;
    }
    http.run().await?;

    Ok(())
}

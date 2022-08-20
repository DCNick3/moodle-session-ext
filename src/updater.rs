use crate::model::{UpdateQueueItem, UpdateQueueKey};
use crate::moodle::SessionUpdateResult;
use crate::{config, Database, Moodle};
use anyhow::Result;
use std::ops::Add;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::select;
use tokio::time::sleep;
use tracing::{debug, info};

pub async fn update_loop(
    db: Arc<Database>,
    moodle: Arc<Moodle>,
    mut watch: kv::Watch<UpdateQueueKey, UpdateQueueItem>,
    config: config::Updater,
) -> Result<()> {
    loop {
        let token = db.get_most_urgent_token()?;

        let now = SystemTime::now();

        let deadline = token
            .as_ref()
            .map(|(_, t)| t.deadline)
            .unwrap_or_else(|| now.add(Duration::from_secs(1000000)));

        if deadline < now + config.gap {
            let (token_id, token) = token.unwrap();

            info!("Updating session {:?}", token_id);

            match moodle
                .update_session(&token.moodle_session, &token.csrf_session)
                .await
            {
                Ok(v) => match v {
                    SessionUpdateResult::Ok { time_left } => {
                        db.update_token(token_id, time_left)?;
                    }
                    SessionUpdateResult::SessionDead => {
                        info!("Session died, removing from db");

                        db.remove_token(token_id)?;
                    }
                },
                Err(e) => {
                    debug!("Session update failed: {}", e);
                }
            }
        } else {
            info!("Nothing to update it seems")
        }

        let timeout = if deadline < now + config.gap {
            Duration::ZERO
        } else {
            deadline.duration_since(now + config.gap).unwrap()
        };

        debug!("Setting a timer for {:?}", timeout);

        select! {
            _ = sleep(timeout) => {
                debug!("Timeout reached, looping");
            },
            _ = &mut watch => {
                debug!("Db update spotted, looping");
            },
        }

        // flush all the updates
        let mut ready = std::future::ready(());
        while select! { biased; _ = &mut watch => true, _ = &mut ready => false } {}
    }
}
use crate::config;
use crate::model::{Email, Token, TokenId, UpdateQueueItem, UpdateQueueKey, User};
use anyhow::Result;
use kv::TransactionError;
use std::fmt::Write;
use std::result;
use std::time::SystemTime;
use tracing::{debug, info, instrument, warn};

pub struct Database {
    db: kv::Store,
    users: kv::Bucket<'static, Email, User>,
    tokens: kv::Bucket<'static, TokenId, Token>,
    update_queue: kv::Bucket<'static, UpdateQueueKey, UpdateQueueItem>,
}

impl Database {
    #[instrument]
    pub fn new(config: config::Database) -> Result<Self> {
        let db = kv::Store::new(kv::Config::new(config.path))?;
        let users = db.bucket(Some("users"))?;
        let tokens = db.bucket(Some("tokens"))?;
        let update_queue = db.bucket(Some("update_queue"))?;

        Ok(Self {
            db,
            users,
            tokens,
            update_queue,
        })
    }

    #[instrument(skip(self))]
    pub fn add_token(&self, email: &Email, moodle_session: String) -> Result<()> {
        self.users.transaction3(
            &self.tokens,
            &self.update_queue,
            |users, tokens, update_queue| {
                let mut user = users.get(email)?.unwrap_or_else(|| {
                    info!("Registered user {}", email.0);
                    User {
                        email: email.clone(),
                        tokens: Vec::new(),
                    }
                });

                let user_tokens = user
                    .tokens
                    .iter()
                    .map(|token_id| -> result::Result<Token, TransactionError<_>> {
                        Ok(tokens
                            .get(token_id)?
                            .expect("User had an invalid token set???"))
                    })
                    .collect::<result::Result<Vec<_>, _>>()?;

                debug!(
                    "Retrieved list of user tokens ({} items)",
                    user_tokens.len()
                );

                if user_tokens
                    .iter()
                    .any(|t| t.moodle_session == moodle_session)
                {
                    // token already stored for this user
                    info!("Token already stored for this user, skipping insertion");
                    return Ok(());
                }

                const MAX_TOKENS_PER_USER: u32 = 3;

                if user_tokens.len() >= MAX_TOKENS_PER_USER as usize {
                    info!(
                        "User reached max of {} tokens; removing the oldest",
                        MAX_TOKENS_PER_USER
                    );

                    let oldest_index = user_tokens
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, t)| t.added)
                        .unwrap()
                        .0;
                    let rm_token = user.tokens.remove(oldest_index);

                    let last_updated = user_tokens[oldest_index].last_updated;

                    let update_queue_key = UpdateQueueKey::from((last_updated, rm_token));

                    assert!(tokens.remove(&rm_token)?.is_some());
                    assert!(update_queue.remove(&update_queue_key)?.is_some());
                }

                let new_token_id = TokenId::from(users.generate_id()?);

                debug!("Will insert the new token with id = {:?}", new_token_id);

                user.tokens.push(new_token_id);

                // a lot of time ago...
                let last_updated = SystemTime::UNIX_EPOCH;
                let added = SystemTime::now();

                let update_queue_key = UpdateQueueKey::from((last_updated, new_token_id));

                let token = Token {
                    owner: email.clone(),
                    moodle_session: moodle_session.clone(),
                    csrf_session: None,
                    last_updated,
                    added,
                };

                assert!(tokens.set(&new_token_id, &token)?.is_none());
                assert!(update_queue
                    .set(
                        &update_queue_key,
                        &UpdateQueueItem {
                            token: new_token_id
                        }
                    )?
                    .is_none());
                users.set(email, &user)?;

                Ok(())
            },
        )?;

        Ok(())
    }

    pub fn dump(&self) -> Result<String> {
        let mut res = String::new();

        writeln!(res, "Users:")?;
        for it in self.users.iter() {
            let it = it?;
            writeln!(res, "{:?} -> {:?}", it.key::<Email>()?, it.value::<User>()?)?;
        }
        writeln!(res, "\n======")?;

        writeln!(res, "Tokens:")?;
        for it in self.tokens.iter() {
            let it = it?;
            writeln!(
                res,
                "{:?} -> {:?}",
                it.key::<TokenId>()?,
                it.value::<Token>()?
            )?;
        }
        writeln!(res, "\n======")?;

        writeln!(res, "Update Queue:")?;
        for it in self.update_queue.iter() {
            let it = it?;
            writeln!(
                res,
                "{:?} -> {:?}",
                it.key::<UpdateQueueKey>()?,
                it.value::<UpdateQueueItem>()?
            )?;
        }
        writeln!(res, "\n======")?;

        Ok(res)
    }
}

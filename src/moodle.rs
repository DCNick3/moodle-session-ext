use crate::{config, Email};
use anyhow::{anyhow, Context, Result};
use email_address::EmailAddress;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderValue, COOKIE, LOCATION};
use reqwest::redirect::Policy;
use reqwest::{Request, Response, Url};
use reqwest_tracing::{
    default_on_request_end, reqwest_otel_span, ReqwestOtelSpanBackend, TracingMiddleware,
};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};
use task_local_extensions::Extensions;
use tracing::{info, instrument};

static EMAIL_EXTRACT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"<dt>Email address</dt><dd><a href="([^"]+)">"#).unwrap());
static SESSION_EXTRACT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""sesskey":"([^"]+)""#).unwrap());

pub struct TimeTrace;

impl ReqwestOtelSpanBackend for TimeTrace {
    fn on_request_start(req: &Request, extension: &mut Extensions) -> tracing::Span {
        extension.insert(Instant::now());
        reqwest_otel_span!(req, time_elapsed_ms = tracing::field::Empty)
    }

    fn on_request_end(
        span: &tracing::Span,
        outcome: &reqwest_middleware::Result<Response>,
        extension: &mut Extensions,
    ) {
        let time_elapsed = extension.get::<Instant>().unwrap().elapsed().as_millis() as i64;
        default_on_request_end(span, outcome);
        span.record("time_elapsed_ms", &time_elapsed);
    }
}

#[derive(Debug)]
pub enum SessionProbeResult {
    Invalid,
    Valid { email: Email, csrf_session: String },
}

#[derive(Debug)]
pub enum SessionUpdateResult {
    SessionDead,
    Ok { time_left: Duration },
}

pub struct Moodle {
    reqwest: reqwest_middleware::ClientWithMiddleware,
    base_url: Url,
    rate_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
}

#[derive(Serialize)]
struct AjaxPayload<T> {
    index: u32,
    methodname: String,
    args: T,
}

#[derive(Debug)]
#[allow(dead_code)]
struct AjaxError {
    pub text: String,
    pub code: String,
}

impl Display for AjaxError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for AjaxError {}

#[derive(Debug)]
enum AjaxResult<T: Deserialize<'static>> {
    Ok(T),
    SessionDead,
    Error(AjaxError),
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SessionTime {
    userid: u64,
    timeremaining: u64,
}

impl Moodle {
    pub fn new(config: config::Moodle) -> Result<Self> {
        let period = Duration::from_millis(1000 * 60 / config.rpm as u64);

        let quota = Quota::with_period(period)
            .unwrap()
            .allow_burst(NonZeroU32::new(config.max_burst).unwrap());

        let rate_limiter = governor::RateLimiter::direct(quota);

        Ok(Self {
            reqwest: reqwest_middleware::ClientBuilder::new(
                reqwest::ClientBuilder::new()
                    .user_agent(config.user_agent)
                    .redirect(Policy::none())
                    .build()?,
            )
            .with(TracingMiddleware::<TimeTrace>::new())
            .build(),
            base_url: config.base_url,
            rate_limiter,
        })
    }

    #[instrument(skip_all)]
    pub async fn check_session(&self, moodle_session: &str) -> Result<SessionProbeResult> {
        self.rate_limiter.until_ready().await;

        let url = self.base_url.join("/user/profile.php")?;

        let resp = self
            .reqwest
            .get(url)
            .header(
                COOKIE,
                HeaderValue::from_str(&format!("MoodleSession={}", moodle_session))?,
            )
            .send()
            .await?;
        if resp.status().is_redirection() {
            info!(
                "Moodle redirected using status {} to {:?}; sessions is likely invalid",
                resp.status(),
                resp.headers().get(LOCATION)
            );
            return Ok(SessionProbeResult::Invalid);
        }

        let body = resp.text().await?;
        let encoded_email = EMAIL_EXTRACT_REGEX
            .captures(&body)
            .context("Could not find email on the profile page")?
            .get(1)
            .unwrap()
            .as_str();

        let email = urlencoding::decode(encoded_email).context("Decoding email")?;
        let email = html_escape::decode_html_entities(&email);
        let email = email
            .strip_prefix("mailto:")
            .context("Stripping mailto prefix")?;

        if !EmailAddress::is_valid(email) {
            return Err(anyhow!(
                "Extracted email address {}, but it seems to be invalid",
                email
            ));
        }

        info!("Session seems to be valid; email = {}", email);

        let sesskey = SESSION_EXTRACT_REGEX
            .captures(&body)
            .context("Could not find sesskey on the profile page")?
            .get(1)
            .unwrap()
            .as_str();

        Ok(SessionProbeResult::Valid {
            email: Email(email.to_string()),
            csrf_session: sesskey.to_string(),
        })
    }

    async fn ajax<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        moodle_session: &str,
        csrf_session: &str,
        method_name: &str,
        args: T,
    ) -> Result<AjaxResult<R>> {
        self.rate_limiter.until_ready().await;

        let url = self
            .base_url
            .join(&format!("/lib/ajax/service.php?sesskey={}", csrf_session))?;

        let resp = self
            .reqwest
            .post(url)
            .header(
                COOKIE,
                HeaderValue::from_str(&format!("MoodleSession={}", moodle_session))?,
            )
            .json(&[AjaxPayload::<T> {
                index: 0,
                methodname: method_name.to_string(),
                args,
            }])
            .send()
            .await?;

        let resp = resp.text().await.context("Reading body as string")?;

        let resp: [serde_json::Map<String, serde_json::Value>; 1] =
            serde_json::from_str(&resp).context("Parsing body as untyped JSON")?;
        let [resp] = resp;
        let error = resp
            .get("error")
            .ok_or_else(|| anyhow!("Missing \"error\" field in response"))?;
        if let Some(err) = error.as_str() {
            let errcode = resp
                .get("errorcode")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing \"errorcode\" field in response or wrong type"))?;
            return Ok(AjaxResult::Error(AjaxError {
                text: err.to_string(),
                code: errcode.to_string(),
            }));
        } else if let Some(true) = error.as_bool() {
            let exception = resp
                .get("exception")
                .and_then(|v| v.as_object())
                .ok_or_else(|| anyhow!("Missing \"exception\" field in response or wrong type"))?;
            let message = exception
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing \"message\" field in exception or wrong type"))?;
            let errorcode = exception
                .get("errorcode")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing \"errorcode\" field in exception or wrong type"))?;

            if errorcode == "servicerequireslogin" {
                return Ok(AjaxResult::SessionDead);
            }

            return Ok(AjaxResult::Error(AjaxError {
                text: message.to_string(),
                code: errorcode.to_string(),
            }));
        }

        let data = resp
            .get("data")
            .ok_or_else(|| anyhow!("Missing \"data\" field in response"))?;

        Ok(AjaxResult::Ok(
            serde_json::from_value(data.clone())
                .context("Parsing response \"data\" field as typed result")?,
        ))
    }

    async fn touch_session(&self, moodle_session: &str, csrf_session: &str) -> Result<bool> {
        Ok(
            match self
                .ajax::<_, bool>(
                    moodle_session,
                    csrf_session,
                    "core_session_touch",
                    serde_json::Map::<String, serde_json::Value>::new(),
                )
                .await
                .context("touch_session")?
            {
                AjaxResult::Ok(v) => {
                    if !v {
                        return Err(anyhow!("`core_session_touch` returned false?????"));
                    }
                    true
                }
                AjaxResult::SessionDead => false,
                AjaxResult::Error(e) => return Err(e).context("core_session_touch"),
            },
        )
    }

    async fn remaining_session_time(
        &self,
        moodle_session: &str,
        csrf_session: &str,
    ) -> Result<Option<SessionTime>> {
        Ok(
            match self
                .ajax::<_, SessionTime>(
                    moodle_session,
                    csrf_session,
                    "core_session_time_remaining",
                    serde_json::Map::<String, serde_json::Value>::new(),
                )
                .await
                .context("remaining_session_time")?
            {
                AjaxResult::Ok(v) => Some(v),
                AjaxResult::SessionDead => None,
                AjaxResult::Error(e) => return Err(e).context("core_session_time_remaining"),
            },
        )
    }

    #[instrument(skip_all)]
    pub async fn update_session(
        &self,
        moodle_session: &str,
        csrf_session: &str,
    ) -> Result<SessionUpdateResult> {
        let touch_result = self
            .touch_session(moodle_session, csrf_session)
            .await
            .context("update_session")?;
        let remaining_time = self
            .remaining_session_time(moodle_session, csrf_session)
            .await
            .context("update_session")?;
        if touch_result {
            if let Some(time) = remaining_time {
                return Ok(SessionUpdateResult::Ok {
                    time_left: Duration::from_secs(time.timeremaining),
                });
            }
        }
        Ok(SessionUpdateResult::SessionDead)
    }
}

use crate::config;
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
    Valid { email: String, csrf_session: String },
}

pub struct Moodle {
    reqwest: reqwest_middleware::ClientWithMiddleware,
    base_url: Url,
    rate_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
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
            email: email.to_string(),
            csrf_session: sesskey.to_string(),
        })
    }
}

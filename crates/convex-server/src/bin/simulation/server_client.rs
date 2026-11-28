//! What the simulators share: their arguments, and the server's HTTP API and stream under
//! the caller's token.
//!
//! The token, when access is required, is read from `CONVEX_SIM_TOKEN`, so it is not on
//! the command line.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
pub use tokio_tungstenite::tungstenite::handshake::client::Request as StreamRequest;

/// The environment variable a bearer token is read from.
const TOKEN_VARIABLE: &str = "CONVEX_SIM_TOKEN";
/// Times a request is sent while no answer comes.
const SENDS: usize = 5;
/// The longest wait for a server too busy for a request.
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// `--name value` pairs.
pub struct Arguments(BTreeMap<String, String>);

impl Arguments {
    /// The process's arguments, each named in `known`.
    pub fn parse(known: &[&str]) -> Result<Self, String> {
        let mut pairs = BTreeMap::new();
        let mut args = std::env::args().skip(1);
        while let Some(name) = args.next() {
            let Some(name) = name.strip_prefix("--") else {
                return Err(format!("expected --name, found {name}"));
            };
            if !known.contains(&name) {
                return Err(format!("--{name} is not an option"));
            }
            let value = args
                .next()
                .ok_or_else(|| format!("--{name} needs a value"))?;
            if pairs.insert(name.to_owned(), value).is_some() {
                return Err(format!("--{name} is given twice"));
            }
        }
        Ok(Self(pairs))
    }

    pub fn required(&self, name: &str) -> Result<&str, String> {
        self.0
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("--{name} is required"))
    }

    /// The value of `--name`, or `default` without one.
    pub fn optional<T: FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.0.get(name) {
            Some(value) => value
                .parse()
                .map_err(|_| format!("--{name} {value} is not valid")),
            None => Ok(default),
        }
    }

    /// The comma-separated values of `--name`, at least one.
    pub fn list(&self, name: &str) -> Result<Vec<String>, String> {
        let values: Vec<String> = self
            .required(name)?
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        if values.is_empty() {
            return Err(format!("--{name} lists nothing"));
        }
        Ok(values)
    }
}

/// The server, as the simulator's identity calls it.
#[derive(Clone)]
pub struct ServerClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl ServerClient {
    /// The server at `base`, `http://host:port`.
    pub fn new(base: &str) -> Result<Self, String> {
        if !base.starts_with("http://") {
            return Err("--server is http://host:port: the server listens on loopback".into());
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            token: std::env::var(TOKEN_VARIABLE).ok(),
        })
    }

    /// A request to `path`, under the token if one is set.
    pub fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let request = self.http.request(method, format!("{}{path}", self.base));
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// A request for the server's stream, under the token if one is set. The server listens
    /// on loopback only, so the stream is plain `ws://`.
    pub fn stream_request(&self) -> Result<StreamRequest, String> {
        let url = format!("ws{}/api/v1/ws", &self.base["http".len()..]);
        let mut request = url
            .into_client_request()
            .map_err(|error| error.to_string())?;
        if let Some(token) = &self.token {
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| "the token is not a header value".to_owned())?;
            request.headers_mut().insert("authorization", value);
        }
        Ok(request)
    }
}

/// Send `request`, and again as it was while no answer comes: no reply, or the server too
/// busy for it saying when to send again (`Retry-After`), after that wait. A refusal that
/// says nothing of when, as a full desk's, is an answer. The server answers a committed
/// write sent again under its idempotency key from the receipt it keeps.
pub async fn send_until_answered(request: RequestBuilder) -> Result<Response, String> {
    let mut failure = String::new();
    for _ in 0..SENDS {
        let attempt = request
            .try_clone()
            .ok_or("a request cannot be sent again")?;
        match attempt.send().await {
            Ok(response) if response.status() == StatusCode::TOO_MANY_REQUESTS => {
                let Some(wait) = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|seconds| seconds.to_str().ok()?.parse().ok())
                    .map(Duration::from_secs)
                else {
                    return Ok(response);
                };
                failure = "the server stayed too busy".into();
                tokio::time::sleep(wait.min(LONGEST_WAIT)).await;
            }
            Ok(response) => return Ok(response),
            Err(error) => failure = error.to_string(),
        }
    }
    Err(format!("a request was not answered: {failure}"))
}

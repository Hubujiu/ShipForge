use super::*;
use std::{cell::RefCell, path::PathBuf, sync::Mutex};
use ureq::{
    Body, SendBody,
    http::{Request, Response},
    middleware::{Middleware, MiddlewareNext},
};

thread_local! {static ACTIVE_DIRECTORY:RefCell<Option<PathBuf>>=const {RefCell::new(None)};}

/// Synchronous provider calls can nest when an adapter rechecks its grants through the client.
/// Capture the calling device without sharing policy or credentials across service threads.
pub(crate) struct ProfileScope(Option<PathBuf>);
impl ProfileScope {
    pub(crate) fn enter(dir: Option<&Path>) -> Self {
        Self(ACTIVE_DIRECTORY.with(|current| current.replace(dir.map(Path::to_owned))))
    }
}
impl Drop for ProfileScope {
    fn drop(&mut self) {
        ACTIVE_DIRECTORY.with(|current| {
            current.replace(self.0.take());
        });
    }
}
struct DestinationPolicy {
    policy: Result<Policy>,
    seconds: u64,
    last: Mutex<Option<(Option<ureq::Proxy>, ureq::Agent)>>,
}
impl Middleware for DestinationPolicy {
    fn handle(
        &self,
        request: Request<SendBody>,
        _: MiddlewareNext,
    ) -> std::result::Result<Response<Body>, ureq::Error> {
        let error = |msg: &str| ureq::Error::Io(std::io::Error::other(msg.to_owned()));
        let url = Url::parse(&request.uri().to_string())
            .map_err(|_| error("invalid provider address"))?;
        let policy = self.policy.as_ref().map_err(|s| error(s))?;
        let route = policy.route(&url).map_err(|s| error(&s))?;
        let agent = {
            let mut last = self
                .last
                .lock()
                .map_err(|_| error("provider transport unavailable"))?;
            if last.as_ref().is_none_or(|(previous, _)| previous != &route) {
                let agent = routed(route.clone(), self.seconds);
                *last = Some((route, agent));
            }
            last.as_ref().unwrap().1.clone()
        };
        // The inner agent owns redirects, timeouts, TLS and the route to this particular destination.
        agent.run(request)
    }
}
pub(crate) fn provider(seconds: u64) -> ureq::Agent {
    let policy = ACTIVE_DIRECTORY
        .with(|dir| dir.borrow().as_deref().map(settings::load).transpose())
        .map(Option::unwrap_or_default);
    ureq::Agent::config_builder()
        .middleware(DestinationPolicy {
            policy,
            seconds,
            last: Mutex::default(),
        })
        .build()
        .new_agent()
}

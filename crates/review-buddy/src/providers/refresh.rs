//! The refresh engine: per-source state machines, backoff with jitter, rate-limit pauses, a
//! per-host concurrency cap, and the clock they all read. Time and randomness are injected, so
//! the logic is deterministic in tests.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rb_core::{Error, SourceId, Timestamp};
use tokio::sync::{oneshot, Notify, Semaphore};

use crate::app::failure::is_server_error;
use crate::app::{SourceFailure, SourceStatus};

pub type Sleep = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Wall-clock time for display and pause deadlines, and sleeping for scheduling.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
    fn sleep(&self, duration: Duration) -> Sleep;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        crate::load::now()
    }

    fn sleep(&self, duration: Duration) -> Sleep {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A clock that only moves when told to. Sleeps finish when [`ManualClock::advance`] passes them.
#[derive(Debug)]
pub struct ManualClock {
    inner: Mutex<ManualInner>,
}

#[derive(Debug)]
struct ManualInner {
    start: Timestamp,
    elapsed: Duration,
    sleepers: Vec<(Duration, oneshot::Sender<()>)>,
}

impl ManualClock {
    pub fn new(start: Timestamp) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ManualInner {
                start,
                elapsed: Duration::ZERO,
                sleepers: Vec::new(),
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, ManualInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Moves time forward and wakes every sleep that is now due.
    pub fn advance(&self, by: Duration) {
        let due = {
            let mut inner = self.lock();
            inner.elapsed += by;
            let now = inner.elapsed;
            let (due, waiting): (Vec<_>, Vec<_>) =
                inner.sleepers.drain(..).partition(|(at, _)| *at <= now);
            inner.sleepers = waiting;
            due
        };
        for (_, wake) in due {
            let _ = wake.send(());
        }
    }

    /// How many sleeps are waiting, so a test can tell the code under test has parked.
    pub fn sleepers(&self) -> usize {
        let mut inner = self.lock();
        inner.sleepers.retain(|(_, wake)| !wake.is_closed());
        inner.sleepers.len()
    }

    /// Yields to the runtime until `n` sleeps are waiting.
    pub async fn parked(&self, n: usize) {
        while self.sleepers() < n {
            tokio::task::yield_now().await;
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        let inner = self.lock();
        Timestamp(inner.start.0 + i64::try_from(inner.elapsed.as_secs()).unwrap_or(i64::MAX))
    }

    fn sleep(&self, duration: Duration) -> Sleep {
        let mut inner = self.lock();
        if duration.is_zero() {
            return Box::pin(async {});
        }
        let (wake, woken) = oneshot::channel();
        let at = inner.elapsed + duration;
        inner.sleepers.push((at, wake));
        Box::pin(async move {
            let _ = woken.await;
        })
    }
}

/// Randomness for jitter: a number in `[0, 1)`.
pub trait Jitter: Send + Sync {
    fn unit(&self) -> f64;
}

/// A small xorshift generator seeded from the clock. Jitter doesn't need to be unpredictable.
#[derive(Debug)]
pub struct SystemJitter(AtomicU64);

impl Default for SystemJitter {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9, |d| d.as_nanos() as u64);
        Self(AtomicU64::new(seed | 1))
    }
}

impl Jitter for SystemJitter {
    fn unit(&self) -> f64 {
        let mut x = self.0.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0.store(x, Ordering::Relaxed);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Always the same number, for tests.
#[derive(Debug, Clone, Copy)]
pub struct FixedJitter(pub f64);

impl Jitter for FixedJitter {
    fn unit(&self) -> f64 {
        self.0
    }
}

/// Exponential backoff for transient failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub cap: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(5),
            cap: Duration::from_secs(300),
        }
    }
}

impl Backoff {
    /// The wait before retry number `attempt` (1 is the first). The delay doubles each time up
    /// to the cap, then half of it is randomised so sources don't retry in step. `unit` is a
    /// number in `[0, 1)`.
    pub fn delay(&self, attempt: u32, unit: f64) -> Duration {
        let doublings = attempt.saturating_sub(1).min(20);
        let raw = self.base.saturating_mul(1u32 << doublings).min(self.cap);
        let half = raw / 2;
        half + half.mul_f64(unit.clamp(0.0, 1.0))
    }
}

/// Spreads `interval` by up to ten per cent either way so several windows don't line up.
pub fn jittered_interval(interval: Duration, unit: f64) -> Duration {
    interval.mul_f64(0.9 + 0.2 * unit.clamp(0.0, 1.0))
}

/// How a failure should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Network trouble or a 5xx: try again with backoff.
    Transient,
    /// The host asked for a pause, with the seconds until it resets if it said.
    RateLimited(Option<u64>),
    /// The token was rejected; retrying won't help.
    Auth,
    Other,
}

pub fn classify(error: &Error) -> Class {
    match error {
        Error::Network { .. } => Class::Transient,
        Error::RateLimited {
            retry_after_secs, ..
        } => Class::RateLimited(*retry_after_secs),
        Error::Unauthorized { .. } => Class::Auth,
        Error::Api(text) if is_server_error(text) => Class::Transient,
        _ => Class::Other,
    }
}

/// What to do after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Wait, then try again. Asking for a refresh yourself cuts the wait short.
    Retry(Duration),
    /// Send nothing until the deadline, whatever is asked.
    Pause {
        until: Timestamp,
    },
    Stop,
}

const DEFAULT_PAUSE_SECS: u64 = 120;

/// One source's refresh state. Pure: it never reads a clock or a random number itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceMachine {
    status: SourceStatus,
    attempt: u32,
    active: bool,
    last_failure: Option<SourceFailure>,
}

impl SourceMachine {
    pub fn status(&self) -> SourceStatus {
        self.status
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn last_failure(&self) -> Option<&SourceFailure> {
        self.last_failure.as_ref()
    }

    /// Starts a cycle. Returns `false` when one is already under way, so requests coalesce.
    pub fn begin(&mut self) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.attempt = 0;
        if !matches!(
            self.status,
            SourceStatus::Offline { .. } | SourceStatus::RateLimited { .. }
        ) {
            self.status = SourceStatus::Refreshing;
        }
        true
    }

    pub fn succeed(&mut self) {
        self.active = false;
        self.attempt = 0;
        self.status = SourceStatus::Ok;
        self.last_failure = None;
    }

    pub fn fail(
        &mut self,
        class: Class,
        failure: SourceFailure,
        now: Timestamp,
        backoff: &Backoff,
        unit: f64,
    ) -> Next {
        self.last_failure = Some(failure);
        match class {
            Class::Transient => {
                self.attempt += 1;
                let since = match self.status {
                    SourceStatus::Offline { since } => since,
                    _ => now,
                };
                self.status = SourceStatus::Offline { since };
                Next::Retry(backoff.delay(self.attempt, unit))
            }
            Class::RateLimited(secs) => {
                let wait = i64::try_from(secs.unwrap_or(DEFAULT_PAUSE_SECS).max(1)).unwrap_or(0);
                let until = Timestamp(now.0 + wait);
                self.status = SourceStatus::RateLimited { until };
                Next::Pause { until }
            }
            Class::Auth => self.stop(SourceStatus::AuthFailed),
            Class::Other => self.stop(SourceStatus::Failed),
        }
    }

    /// The host is paused for everyone: wait without sending anything.
    pub fn pause_until(&mut self, until: Timestamp) {
        self.status = SourceStatus::RateLimited { until };
    }

    fn stop(&mut self, status: SourceStatus) -> Next {
        self.active = false;
        self.status = status;
        Next::Stop
    }
}

/// Where the engine's state lives: machines, host semaphores and pauses.
pub struct Engine {
    pub clock: Arc<dyn Clock>,
    pub jitter: Arc<dyn Jitter>,
    pub backoff: Backoff,
    pub focus_gap: Duration,
    per_host: usize,
    hosts: Mutex<HashMap<String, Arc<Semaphore>>>,
    paused: Mutex<HashMap<String, Timestamp>>,
    machines: Mutex<HashMap<SourceId, SourceMachine>>,
    kicks: Mutex<HashMap<SourceId, Arc<Notify>>>,
    last_started: Mutex<Option<Timestamp>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("per_host", &self.per_host)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    pub fn new(per_host: usize) -> Self {
        Self {
            clock: Arc::new(SystemClock),
            jitter: Arc::new(SystemJitter::default()),
            backoff: Backoff::default(),
            focus_gap: Duration::from_secs(30),
            per_host: per_host.max(1),
            hosts: Mutex::default(),
            paused: Mutex::default(),
            machines: Mutex::default(),
            kicks: Mutex::default(),
            last_started: Mutex::default(),
        }
    }

    /// One semaphore per host, shared by every source on it.
    pub fn semaphore(&self, host: &str) -> Arc<Semaphore> {
        let mut hosts = lock(&self.hosts);
        Arc::clone(
            hosts
                .entry(host.to_ascii_lowercase())
                .or_insert_with(|| Arc::new(Semaphore::new(self.per_host))),
        )
    }

    pub fn begin(&self, source: &SourceId) -> bool {
        lock(&self.machines)
            .entry(source.clone())
            .or_default()
            .begin()
    }

    pub fn with_machine<R>(&self, source: &SourceId, f: impl FnOnce(&mut SourceMachine) -> R) -> R {
        f(lock(&self.machines).entry(source.clone()).or_default())
    }

    pub fn status(&self, source: &SourceId) -> SourceStatus {
        self.with_machine(source, |m| m.status())
    }

    pub fn kick(&self, source: &SourceId) -> Arc<Notify> {
        Arc::clone(
            lock(&self.kicks)
                .entry(source.clone())
                .or_insert_with(|| Arc::new(Notify::new())),
        )
    }

    pub fn pause_host(&self, host: &str, until: Timestamp) {
        let mut paused = lock(&self.paused);
        let entry = paused.entry(host.to_ascii_lowercase()).or_insert(until);
        *entry = (*entry).max(until);
    }

    /// When the host may be asked again, if it is paused now.
    pub fn host_paused_until(&self, host: &str) -> Option<Timestamp> {
        let now = self.clock.now();
        let mut paused = lock(&self.paused);
        let key = host.to_ascii_lowercase();
        match paused.get(&key) {
            Some(until) if *until > now => Some(*until),
            Some(_) => {
                paused.remove(&key);
                None
            }
            None => None,
        }
    }

    /// Records a refresh starting now. Returns `false` if `min_gap` hasn't passed since the last.
    pub fn mark_started(&self, min_gap: Option<Duration>) -> bool {
        let now = self.clock.now();
        let mut last = lock(&self.last_started);
        if let (Some(gap), Some(at)) = (min_gap, *last) {
            if now.0 - at.0 < i64::try_from(gap.as_secs()).unwrap_or(i64::MAX) {
                return false;
            }
        }
        *last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::FailureKind;

    fn failure() -> SourceFailure {
        SourceFailure::unavailable("x", "y")
    }

    fn fail(m: &mut SourceMachine, class: Class, now: i64) -> Next {
        m.fail(class, failure(), Timestamp(now), &Backoff::default(), 0.0)
    }

    #[test]
    fn backoff_doubles_to_the_cap_and_never_overflows() {
        let b = Backoff::default();
        let at = |n| b.delay(n, 1.0);
        assert_eq!(at(1), Duration::from_secs(5));
        assert_eq!(at(2), Duration::from_secs(10));
        assert_eq!(at(3), Duration::from_secs(20));
        assert_eq!(at(7), Duration::from_secs(300));
        assert_eq!(at(1000), Duration::from_secs(300));
        assert_eq!(b.delay(0, 1.0), Duration::from_secs(5));
    }

    #[test]
    fn jitter_keeps_half_the_delay_and_adds_up_to_the_rest() {
        let b = Backoff::default();
        assert_eq!(b.delay(3, 0.0), Duration::from_secs(10));
        assert_eq!(b.delay(3, 0.5), Duration::from_secs(15));
        assert_eq!(b.delay(3, 1.0), Duration::from_secs(20));
        assert_eq!(b.delay(3, 7.0), Duration::from_secs(20));
    }

    #[test]
    fn interval_jitter_stays_within_ten_per_cent() {
        let i = Duration::from_secs(100);
        assert_eq!(jittered_interval(i, 0.0), Duration::from_secs(90));
        assert_eq!(jittered_interval(i, 0.5), Duration::from_secs(100));
        assert_eq!(jittered_interval(i, 1.0), Duration::from_secs(110));
    }

    #[test]
    fn system_jitter_stays_in_range() {
        let j = SystemJitter::default();
        for _ in 0..1000 {
            let u = j.unit();
            assert!((0.0..1.0).contains(&u));
        }
    }

    #[test]
    fn errors_are_classified() {
        let host = || "h".to_string();
        assert_eq!(
            classify(&Error::Network {
                host: host(),
                reason: "x".into()
            }),
            Class::Transient
        );
        assert_eq!(
            classify(&Error::Api("h answered 502: bad gateway".into())),
            Class::Transient
        );
        assert_eq!(classify(&Error::Api("h answered 418".into())), Class::Other);
        assert_eq!(
            classify(&Error::RateLimited {
                host: host(),
                retry_after_secs: Some(9)
            }),
            Class::RateLimited(Some(9))
        );
        assert_eq!(classify(&Error::Unauthorized { host: host() }), Class::Auth);
        assert_eq!(classify(&Error::NotFound("x".into())), Class::Other);
    }

    #[test]
    fn a_cycle_coalesces_until_it_settles() {
        let mut m = SourceMachine::default();
        assert!(m.begin());
        assert_eq!(m.status(), SourceStatus::Refreshing);
        assert!(!m.begin());
        m.succeed();
        assert_eq!(m.status(), SourceStatus::Ok);
        assert!(m.begin());
    }

    #[test]
    fn transient_failures_go_offline_and_keep_the_first_time() {
        let mut m = SourceMachine::default();
        m.begin();
        assert_eq!(
            fail(&mut m, Class::Transient, 100),
            Next::Retry(Duration::from_secs(2) + Duration::from_millis(500))
        );
        assert_eq!(
            m.status(),
            SourceStatus::Offline {
                since: Timestamp(100)
            }
        );
        fail(&mut m, Class::Transient, 200);
        assert_eq!(
            m.status(),
            SourceStatus::Offline {
                since: Timestamp(100)
            }
        );
        assert!(m.is_active());
        m.succeed();
        assert_eq!(m.status(), SourceStatus::Ok);
        assert!(m.last_failure().is_none());
    }

    #[test]
    fn rate_limits_pause_until_the_reset() {
        let mut m = SourceMachine::default();
        m.begin();
        assert_eq!(
            fail(&mut m, Class::RateLimited(Some(90)), 1000),
            Next::Pause {
                until: Timestamp(1090)
            }
        );
        assert_eq!(
            m.status(),
            SourceStatus::RateLimited {
                until: Timestamp(1090)
            }
        );
        assert_eq!(
            fail(&mut m, Class::RateLimited(None), 1000),
            Next::Pause {
                until: Timestamp(1120)
            }
        );
    }

    #[test]
    fn auth_and_other_failures_stop_the_cycle() {
        let mut m = SourceMachine::default();
        m.begin();
        assert_eq!(fail(&mut m, Class::Auth, 1), Next::Stop);
        assert_eq!(m.status(), SourceStatus::AuthFailed);
        assert!(!m.is_active());
        m.begin();
        assert_eq!(fail(&mut m, Class::Other, 1), Next::Stop);
        assert_eq!(m.status(), SourceStatus::Failed);
        assert_eq!(
            m.last_failure().map(|f| f.kind),
            Some(FailureKind::Unavailable)
        );
    }

    #[tokio::test]
    async fn the_host_semaphore_is_shared_and_capped() {
        let engine = Engine::new(2);
        let a = engine.semaphore("GitHub.com");
        let b = engine.semaphore("github.com");
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &engine.semaphore("gitlab.com")));
        let _p1 = a.clone().acquire_owned().await.unwrap();
        let _p2 = b.clone().acquire_owned().await.unwrap();
        assert!(a.try_acquire().is_err());
        assert!(engine.semaphore("gitlab.com").try_acquire().is_ok());
    }

    #[tokio::test]
    async fn the_manual_clock_wakes_sleeps_only_when_advanced() {
        let clock = ManualClock::new(Timestamp(1000));
        let sleep = clock.sleep(Duration::from_secs(10));
        assert_eq!(clock.sleepers(), 1);
        clock.advance(Duration::from_secs(9));
        assert_eq!(clock.sleepers(), 1);
        assert_eq!(clock.now(), Timestamp(1009));
        clock.advance(Duration::from_secs(1));
        sleep.await;
        assert_eq!(clock.sleepers(), 0);
    }

    #[test]
    fn host_pauses_expire_with_the_clock() {
        let clock = ManualClock::new(Timestamp(0));
        let mut engine = Engine::new(4);
        engine.clock = clock.clone();
        engine.pause_host("h", Timestamp(60));
        assert_eq!(engine.host_paused_until("H"), Some(Timestamp(60)));
        clock.advance(Duration::from_secs(60));
        assert_eq!(engine.host_paused_until("h"), None);
    }

    #[test]
    fn the_focus_gap_is_enforced_from_the_last_start() {
        let clock = ManualClock::new(Timestamp(0));
        let mut engine = Engine::new(4);
        engine.clock = clock.clone();
        let gap = Some(Duration::from_secs(30));
        assert!(engine.mark_started(gap));
        clock.advance(Duration::from_secs(29));
        assert!(!engine.mark_started(gap));
        clock.advance(Duration::from_secs(1));
        assert!(engine.mark_started(gap));
        assert!(engine.mark_started(None));
    }
}

use std::{collections::HashMap, net::IpAddr, time::Instant};
use std::sync::Mutex;

// stub: il Task 3 la completa (check/fail/success/reap con orologio iniettabile)
#[allow(dead_code)]
pub struct RateLimiter {
    fails: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl RateLimiter {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            fails: Mutex::new(HashMap::new()),
        }
    }
}

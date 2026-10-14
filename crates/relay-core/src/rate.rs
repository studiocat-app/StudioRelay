/// Returned when a bucket is empty: how long until enough tokens refill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimited {
    pub retry_after_ms: u64,
}

/// A token bucket driven by the host's clock (milliseconds).
#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: f64,
    per_ms: f64,
    tokens: f64,
    updated_ms: u64,
}

impl TokenBucket {
    /// `capacity` is the burst size; `per_second` the steady refill rate.
    pub fn new(capacity: u32, per_second: f64, now_ms: u64) -> Self {
        Self {
            capacity: capacity as f64,
            per_ms: per_second / 1000.0,
            tokens: capacity as f64,
            updated_ms: now_ms,
        }
    }

    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.updated_ms) as f64;
        self.tokens = (self.tokens + elapsed * self.per_ms).min(self.capacity);
        self.updated_ms = self.updated_ms.max(now_ms);
    }

    /// Takes `n` tokens, or says when to retry. Never takes a partial amount.
    pub fn try_take(&mut self, now_ms: u64, n: u32) -> Result<(), RateLimited> {
        self.refill(now_ms);
        let n = n as f64;
        if self.tokens >= n {
            self.tokens -= n;
            return Ok(());
        }
        let missing = n - self.tokens;
        let retry_after_ms = if self.per_ms > 0.0 {
            (missing / self.per_ms).ceil() as u64
        } else {
            u64::MAX
        };
        Err(RateLimited { retry_after_ms })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_refill() {
        let mut b = TokenBucket::new(2, 1.0, 0);
        assert!(b.try_take(0, 1).is_ok());
        assert!(b.try_take(0, 1).is_ok());
        assert_eq!(
            b.try_take(0, 1),
            Err(RateLimited {
                retry_after_ms: 1000
            })
        );
        assert!(b.try_take(1000, 1).is_ok());
    }

    #[test]
    fn refill_never_exceeds_capacity() {
        let mut b = TokenBucket::new(3, 10.0, 0);
        assert!(b.try_take(1_000_000, 3).is_ok());
        assert!(b.try_take(1_000_000, 1).is_err());
    }

    #[test]
    fn clock_going_backwards_does_not_mint_tokens() {
        let mut b = TokenBucket::new(1, 1.0, 5_000);
        assert!(b.try_take(5_000, 1).is_ok());
        assert!(b.try_take(1_000, 1).is_err());
    }
}

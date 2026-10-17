//! Data parallelism with the standard library only.
//!
//! The engine's work is thousands of independent files: read, parse, extract.
//! `par_map` spreads them over every core. Workers take the next item from a
//! shared counter, so one huge file does not hold a whole slice of others back.
//! Results come back in the ORDER OF THE INPUT, so every report built from them
//! is the same run after run, exactly as when it was sequential.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// Threads to use: the machine's parallelism, overridable with
/// `KEBITX_THREADS` (1 = sequential, for debugging and for comparing).
pub fn threads() -> usize {
    std::env::var("KEBITX_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| {
            thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        })
}

/// `items.iter().map(f).collect()`, on every core, results in input order.
pub fn par_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let workers = threads().min(items.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut parts: Vec<Vec<(usize, R)>> = thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        out.push((i, f(item)));
                    }
                    out
                })
            })
            .collect();
        // a panic in a worker is a bug: propagate it, never lose results quietly
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let mut all: Vec<(usize, R)> = Vec::with_capacity(items.len());
    for p in parts.iter_mut() {
        all.append(p);
    }
    all.sort_unstable_by_key(|(i, _)| *i);
    all.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_result_and_order_as_sequential() {
        let items: Vec<u64> = (0..10_000).collect();
        let f = |x: &u64| x * x % 977;
        assert_eq!(par_map(&items, f), items.iter().map(f).collect::<Vec<_>>());
        assert_eq!(par_map(&[] as &[u64], f), Vec::<u64>::new());
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs async core APIs from synchronous front ends (the CLI).

use std::future::Future;

/// Runs `f` to completion on a fresh current-thread Tokio runtime.
pub fn block_on<F: Future>(f: F) -> std::io::Result<F::Output> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(rt.block_on(f))
}

#[cfg(test)]
mod tests {
    #[test]
    fn runs_a_future_with_timers() {
        let v = super::block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            7
        })
        .unwrap();
        assert_eq!(v, 7);
    }
}

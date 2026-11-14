// Tests numerical ordering in the slow release series.
// Cases reject leading zeroes; see `src/distribution/update.rs`.
use super::Version;
#[test]
fn slow_series_orders_numerically_and_rejects_leading_zeroes() {
    let v = |s| Version::parse(s).unwrap();
    assert!(v("v0.31.28").cmp(&v("v0.31.0")).is_gt());
    assert!(v("v0.31.28").cmp(&v("v0.31.9")).is_gt());
    assert!(v("v0.31.99").cmp(&v("v0.31.28")).is_gt());
    assert!(v("v0.32.10").cmp(&v("v0.31.99")).is_gt());
    for invalid in ["v0.31.01", "v0.31.09", "v0.31.010", "v0.031.10"] {
        assert!(Version::parse(invalid).is_none());
    }
}

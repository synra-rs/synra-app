use std::sync::Arc;

use synra_store::SynraDb;

/// Points the store's write timer at the shell's recorder.
///
/// # Why this is a named function
///
/// Because the metric's name and its label are one decision, and a test that wants to see a write counted should call the
/// same function the shell calls — not build a second histogram with the same name and hope the two agree. The daemon has
/// the same function over the same store hook; the hosts differ in where their recorder comes from, not in what a write
/// reports.
pub fn observe_db_writes(db: &mut SynraDb, metrics: &Arc<synra_observe::metrics::RingRecorder>) {
    let recorder = Arc::clone(metrics);
    db.observe_writes(Arc::new(move |kind, elapsed| {
        recorder.histogram_observe(
            &synra_observe::metrics::SeriesKey::new(
                "db_write_duration_us",
                [("kind".to_owned(), kind.to_owned())],
            ),
            elapsed.as_secs_f64() * 1_000_000.0,
        );
    }));
}

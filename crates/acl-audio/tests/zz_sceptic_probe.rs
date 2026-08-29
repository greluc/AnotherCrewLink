use acl_audio::resample::Resampler;

#[test]
fn probe_chunk_burst() {
    for rate in [8000u32, 16000, 44100, 48000] {
        let mut r = Resampler::new(rate, 960).expect("resampler");
        let ten_ms = (rate as usize) / 100;
        let mut sizes = Vec::new();
        let mut first_nonempty_after = None;
        let mut pushes = 0usize;
        for i in 0..60 {
            let mut out = Vec::new();
            r.push(&vec![0.1f32; ten_ms], &mut out).expect("push");
            pushes += 1;
            if !out.is_empty() {
                if first_nonempty_after.is_none() {
                    first_nonempty_after = Some(i + 1);
                }
                sizes.push(out.len());
            }
        }
        let _ = pushes;
        println!(
            "rate {rate}: chunk_ms={:.1} first_nonempty_after_pushes={:?} nonempty_sizes={:?} frames_each={:?}",
            960.0 * 1000.0 / rate as f64,
            first_nonempty_after,
            &sizes[..sizes.len().min(6)],
            sizes.iter().take(6).map(|s| *s as f64 / 960.0).collect::<Vec<_>>()
        );
    }
}

use criterion::{criterion_group, criterion_main, Criterion};
use rewatch::build;

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("clean-build", |b| {
        b.iter(|| {
            let folder = "testrepo";
            build::clean(folder);
            let _ = build::build(folder);
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

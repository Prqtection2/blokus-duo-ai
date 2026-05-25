use criterion::{criterion_group, criterion_main, Criterion};

fn version_bench(c: &mut Criterion) {
    c.bench_function("version", |b| b.iter(blokus_core::version));
}

criterion_group!(benches, version_bench);
criterion_main!(benches);

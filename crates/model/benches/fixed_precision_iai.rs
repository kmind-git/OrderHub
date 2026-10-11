// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_model::types::fixed::{f64_to_fixed_i64, f64_to_fixed_i128};
use std::hint::black_box;

fn bench_fixed_i64_precision() -> i64 {
    f64_to_fixed_i64(black_box(-0.000_000_001), black_box(9))
}

fn bench_fixed_i128_precision() -> i128 {
    f64_to_fixed_i128(black_box(-0.000_000_001), black_box(9))
}

// i64 operations
fn bench_i64_add() -> i64 {
    let a = black_box(1_000_000_000); // 1 billion
    let b = black_box(2_000_000_000); // 2 billion
    a + b
}

fn bench_i64_sub() -> i64 {
    let a = black_box(2_000_000_000);
    let b = black_box(1_000_000_000);
    a - b
}

fn bench_i64_mul() -> i64 {
    let a = black_box(1_000_000);
    let b = black_box(1_000);
    a * b
}

fn bench_i64_div() -> i64 {
    let a = black_box(1_000_000_000);
    let b = black_box(1_000);
    a / b
}

// i128 operations
fn bench_i128_add() -> i128 {
    let a = black_box(1_000_000_000_i128);
    let b = black_box(2_000_000_000_i128);
    a + b
}

fn bench_i128_sub() -> i128 {
    let a = black_box(2_000_000_000_i128);
    let b = black_box(1_000_000_000_i128);
    a - b
}

fn bench_i128_mul() -> i128 {
    let a = black_box(1_000_000_i128);
    let b = black_box(1_000_i128);
    a * b
}

fn bench_i128_div() -> i128 {
    let a = black_box(1_000_000_000_i128);
    let b = black_box(1_000_i128);
    a / b
}

// These historical filenames now measure elapsed time, not Valgrind instructions.
fn benchmarks(c: &mut Criterion) {
    c.bench_function("bench_i64_add", |b| {
        b.iter(bench_i64_add);
    });
    c.bench_function("bench_i64_sub", |b| {
        b.iter(bench_i64_sub);
    });
    c.bench_function("bench_i64_mul", |b| {
        b.iter(bench_i64_mul);
    });
    c.bench_function("bench_i64_div", |b| {
        b.iter(bench_i64_div);
    });
    c.bench_function("bench_i128_add", |b| {
        b.iter(bench_i128_add);
    });
    c.bench_function("bench_i128_sub", |b| {
        b.iter(bench_i128_sub);
    });
    c.bench_function("bench_i128_mul", |b| {
        b.iter(bench_i128_mul);
    });
    c.bench_function("bench_i128_div", |b| {
        b.iter(bench_i128_div);
    });
    c.bench_function("bench_fixed_i64_precision", |b| {
        b.iter(bench_fixed_i64_precision);
    });
    c.bench_function("bench_fixed_i128_precision", |b| {
        b.iter(bench_fixed_i128_precision);
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
